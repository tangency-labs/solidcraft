//! A drawing view's shaded pictures (W17b S4, #5624): which pictures a view draws and how each body
//! takes part in each, from its two visual styles and whether reference parts hide the model.
//!
//! - The model is drawn when its style is shaded, reference parts
//!   ([`crate::drawing_view::Shown::reference`]) when theirs is.
//! - The model always hides reference parts behind it; reference parts hide the model only when
//!   `reference_hides_model` is on.
//! - Shaded reference parts that don't hide the model get a picture of their own, drawn first,
//!   under the model's, which leaves them out: so they never cover it.
//!
//! A view's group holds its pictures under its lines ([`picture_items`]), and [`ViewPictures`] has
//! their pixels for a writer: of the parts the view shows, placed by the
//! [`solidcraft_geom::drawing_view::ViewFrame`] that placed the lines. Nothing is kept between exports:
//! each renders again. The sheet page's pictures are drawn a band at a time by the sheet scene's
//! provider ([`crate::drawing_scene::step`]) and kept with their view's lines. The files and the page
//! find a view's pictures by one rule ([`view_pictures`]) and draw each the one way ([`PictureRows`]).

use std::cell::{Cell, OnceCell};
use std::collections::BTreeMap;

use solidcore_record::Vec2;
use solidcraft_doc::drawing::Drawing;
use solidcraft_doc::drawing::view::ResolvedView;
use solidcraft_doc::ids;
use solidcraft_geom::Aabb2;
use solidcraft_geom::drawing_view::ViewFrame;
use solidcraft_geom::paper::{Item, Picture, Pictures};
use solidcraft_geom::pixel_grid::{GridWindow, PixelGrid};
use solidcraft_render::job::{
    BodyRole, CancelToken, MAX_PICTURE_PIXEL_TESTS, OrthoFrame, Phase, PicturePoll, PictureRequest, PictureTask, PixelWindow, Projection, Supersample,
};
use solidcraft_render::{ProductScene, VisualStyle};

use crate::drawing_view::{ShownSet, Styles, Unmeasured};
use crate::params::bad;
use crate::{Result, Session};

const CMD: &str = "drawing.export";
/// Marks the picture under a view's top one: the top one has the view's own id as its source.
const UNDER: u64 = 0x5555_5555_5555_5555;

/// How a body takes part in one picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// Drawn in its own colours.
    Painted,
    /// Hides what lies behind it and draws nothing.
    DepthOnly,
    /// Not in the picture.
    Left,
}

/// The widest and tallest window a picture task is given to draw at once.
const TILE: u32 = 1024;

/// The pictures a view draws, bottom first, each as one [`Part`] for every body, where
/// `is_reference[i]` says whether body `i` is a reference part. Empty when nothing shaded is there.
pub fn pictures(display: VisualStyle, reference: VisualStyle, hides_model: bool, is_reference: &[bool]) -> Vec<Vec<Part>> {
    let (model, references) = (display.is_shaded(), reference.is_shaded());
    let picture = |part: fn(bool, bool) -> Part| is_reference.iter().map(|r| part(*r, hides_model)).collect::<Vec<_>>();
    let pictures = match (model, references) {
        (false, false) => vec![],
        // Each hides the other: one picture. Otherwise the references' own, under the model's.
        (true, true) if hides_model => vec![picture(|_, _| Part::Painted)],
        (true, true) => vec![picture(|r, _| if r { Part::Painted } else { Part::Left }), picture(|r, _| if r { Part::Left } else { Part::Painted })],
        // Lines-only reference parts hide the model's picture only when they hide the model.
        (true, false) => vec![picture(|r, hides| match (r, hides) {
            (false, _) => Part::Painted,
            (true, true) => Part::DepthOnly,
            (true, false) => Part::Left,
        })],
        // A lines-only model still hides the reference parts behind it.
        (false, true) => vec![picture(|r, _| if r { Part::Painted } else { Part::DepthOnly })],
    };
    pictures.into_iter().filter(|p| p.contains(&Part::Painted)).collect()
}

/// [`pictures`] for the bodies `set` holds, in its order.
pub fn view_parts(set: &ShownSet, styles: Styles) -> Vec<Vec<Part>> {
    let flags: Vec<bool> = set.bodies.iter().map(|b| b.reference).collect();
    pictures(styles.display, styles.reference, styles.reference_hides_model, &flags)
}

/// The source of a view's picture `index` of `count`, bottom first.
fn source(view: u64, index: usize, count: usize) -> u64 {
    if index + 1 == count { view } else { view ^ UNDER }
}

/// View `view`'s pictures as scene items over `rect`, its boundary, bottom first.
pub fn picture_items(view: u64, parts: &[Vec<Part>], rect: Aabb2) -> Vec<Item> {
    (0..parts.len()).map(|i| Item::Picture(Picture { source: source(view, i, parts.len()), rect, opacity: 1.0 })).collect()
}

/// The bodies one picture draws, of those `chosen` holds with their `parts`: each by its place among
/// the bodies the design places ([`crate::drawing_view::Shown::index`]), with its role.
fn drawn_bodies(chosen: &ShownSet, parts: Vec<Part>) -> Vec<(usize, BodyRole)> {
    let role = |part| match part {
        Part::Painted => Some(BodyRole::Painted),
        Part::DepthOnly => Some(BodyRole::DepthOnly),
        Part::Left => None,
    };
    chosen.bodies.iter().zip(parts).filter_map(|(body, part)| Some((body.index, role(part)?))).collect()
}

/// What placed a view's lines, as the renderer takes it.
fn ortho(frame: &ViewFrame) -> OrthoFrame {
    let basis = frame.basis();
    OrthoFrame { basis, scale: frame.scale().factor(), shift: frame.to_paper(basis.origin) }
}

/// One picture of a view: how it is placed, and the bodies it draws.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ViewPicture {
    pub(crate) view: u64,
    pub(crate) frame: OrthoFrame,
    /// The bodies it draws ([`drawn_bodies`]).
    pub(crate) bodies: Vec<(usize, BodyRole)>,
}

/// View `id`'s pictures, resolved as `view` and centred on `centre`, by source, bottom first: of the
/// bodies it shows of `shown`, placed as its lines are ([`crate::drawing_view::measured`]), each with
/// the bodies it draws ([`drawn_bodies`]). None when it has nothing to measure, or its lines say why it
/// isn't drawn ([`crate::drawing_view::not_drawable_apart`]).
pub(crate) fn view_pictures(
    shown: &std::result::Result<ShownSet, Unmeasured>,
    id: u64,
    view: &ResolvedView,
    centre: Vec2,
) -> Result<Vec<(u64, ViewPicture)>> {
    if shown.is_err() || crate::drawing_view::not_drawable_apart(view).is_some() {
        return Ok(Vec::new());
    }
    let Ok((chosen, extents)) = crate::drawing_view::measured(shown, view) else { return Ok(Vec::new()) };
    let frame = ortho(&crate::drawing_view::frame(view.basis, view.scale.value, &extents.model, centre, CMD)?);
    let pictures = view_parts(&chosen, Styles::from(view));
    let count = pictures.len();
    let picture = |(i, parts): (usize, Vec<Part>)| (source(id, i, count), ViewPicture { view: id, frame, bodies: drawn_bodies(&chosen, parts) });
    Ok(pictures.into_iter().enumerate().map(picture).collect())
}

/// What is said of a view whose shaded pictures cost more than `max_pixel_tests` pixel tests in all.
pub(crate) fn over_budget(max_pixel_tests: u64) -> String {
    format!("its shaded pictures exceed {max_pixel_tests} pixel tests; reduce its scale or what it shows")
}

/// One picture being drawn into the rows of a window of its grid, a band at a time: what the files
/// and the sheet page draw a picture with, so they draw the same pixels.
pub(crate) struct PictureRows {
    task: PictureTask,
    window: GridWindow,
    /// The window's rows so far: straight alpha sRGB RGBA, top row first. Made at the first band, so
    /// a picture refused for its cost is never given them.
    pixels: Vec<u8>,
}

impl PictureRows {
    /// `picture` of the bodies `whole` holds (every body of the world state, each at its place) on
    /// `grid`, for the columns and rows of `window`.
    pub(crate) fn new(whole: &ProductScene, picture: &ViewPicture, grid: &PixelGrid, window: GridWindow) -> std::result::Result<PictureRows, String> {
        // Each body's mesh by its place: `drawing_view::shown` and `extract_where` walk the world
        // state's bodies in the same order.
        let meshes = picture.bodies.iter().map(|(index, _)| whole.meshes.get(*index).cloned()).collect::<Option<Vec<_>>>();
        let meshes = meshes.ok_or_else(|| "the model changed while it was drawn".to_string())?;
        let roles = picture.bodies.iter().map(|(_, role)| *role).collect();
        let request = PictureRequest {
            projection: Projection::Ortho { frame: picture.frame, grid: *grid },
            tiles: tiles(window),
            supersample: Supersample::default(),
            roles,
            background: None,
        };
        let scene = ProductScene { meshes, bounds: whole.bounds };
        let task = PictureTask::new(scene, request, CancelToken::default()).map_err(|e| e.to_string())?;
        Ok(PictureRows { task, window, pixels: Vec::new() })
    }

    /// The pixel tests drawing it costs, counted before it starts.
    pub(crate) fn pixel_tests(&self) -> u64 {
        self.task.projected_pixel_tests()
    }

    /// Places every triangle before the first band, a unit at a time so it stops where drawing starts.
    /// The renderer counts a triangle's placing as one unit, as it does a pixel test, but placing one
    /// counts its pixel tests row by row: a band of them takes as long as the whole picture's setup.
    pub(crate) fn prepare(&mut self) -> std::result::Result<(), String> {
        while self.preparing() {
            self.task.step(1).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Whether its triangles are still being placed.
    pub(crate) fn preparing(&self) -> bool {
        self.task.progress().phase == Phase::Preparing
    }

    /// Draws one band, the renderer's own bound of work, and gives the rows once every band is drawn.
    pub(crate) fn band(&mut self) -> std::result::Result<Option<Vec<u8>>, String> {
        let (window, width) = (self.window, (self.window.x1 - self.window.x0) as usize);
        if self.pixels.is_empty() {
            self.pixels = vec![0; width * (window.y1 - window.y0) as usize * 4];
        }
        match self.task.step(16_384).map_err(|e| e.to_string())? {
            PicturePoll::Pending(_) => {}
            PicturePoll::Tile(tile, rows) => {
                let stride = (tile.x1 - tile.x0) as usize * 4;
                for (row, line) in rows.chunks_exact(stride).enumerate() {
                    let at = (((tile.y0 - window.y0) as usize + row) * width + (tile.x0 - window.x0) as usize) * 4;
                    let into = self.pixels.get_mut(at..at + stride).ok_or("a tile outside its window")?;
                    into.copy_from_slice(line);
                }
            }
            PicturePoll::Complete => return Ok(Some(std::mem::take(&mut self.pixels))),
        }
        Ok(None)
    }
}

/// The pixels of the shaded views' pictures on one sheet, for the writers.
pub struct ViewPictures<'a> {
    session: &'a Session,
    /// Each picture, by its source.
    pictures: BTreeMap<u64, ViewPicture>,
    /// Every body the design places, read the first time a writer asks.
    scene: OnceCell<std::result::Result<ProductScene, String>>,
    /// The pixel tests each view's requests have counted: a writer asks for a picture a band at a
    /// time, and this is what bounds the whole view.
    spent: BTreeMap<u64, Cell<u64>>,
    max_pixel_tests: u64,
}

impl<'a> ViewPictures<'a> {
    /// The pictures of the views on sheet `sheet` of `d`, each placed as its lines are. With nothing
    /// to show, a view is a placeholder and has none.
    pub fn new(s: &'a Session, d: &Drawing, sheet: u64) -> Result<ViewPictures<'a>> {
        let (mut pictures, mut spent) = (BTreeMap::new(), BTreeMap::new());
        let shown = crate::drawing_view::shown(s);
        for (id, view, centre) in crate::drawing_scene::views_on(d, sheet)? {
            for (source, picture) in view_pictures(&shown, id, &view, centre)? {
                spent.entry(id).or_default();
                if pictures.insert(source, picture).is_some() {
                    return Err(bad(CMD, format!("view {}: its picture's number is another's", ids::encode(id))));
                }
            }
        }
        Ok(ViewPictures { session: s, pictures, scene: OnceCell::new(), spent, max_pixel_tests: MAX_PICTURE_PIXEL_TESTS })
    }

    /// Whether `source` is one of these pictures.
    pub fn has(&self, source: u64) -> bool {
        self.pictures.contains_key(&source)
    }
}

/// `window` in tiles a picture task takes: a sheet at a high dpi is far wider than one may be.
fn tiles(window: GridWindow) -> Vec<PixelWindow> {
    let along = |from: u32, to: u32| (from..to).step_by(TILE as usize).map(move |start| (start, (start + TILE).min(to)));
    along(window.y0, window.y1).flat_map(|(y0, y1)| along(window.x0, window.x1).map(move |(x0, x1)| PixelWindow { x0, y0, x1, y1 })).collect()
}

impl Pictures for ViewPictures<'_> {
    fn rows(&self, source: u64, grid: &PixelGrid, window: GridWindow) -> std::result::Result<Vec<u8>, String> {
        let picture = self.pictures.get(&source).ok_or_else(|| format!("no pixels for picture {source}"))?;
        let named = |why: String| format!("view {}: {why}", ids::encode(picture.view));
        let extract = || crate::product_scene::extract_where(self.session, |_| true).map_err(|e| e.to_string());
        let whole = self.scene.get_or_init(extract).as_ref().map_err(|why| named(why.clone()))?;
        let mut rows = PictureRows::new(whole, picture, grid, window).map_err(named)?;
        let spent = self.spent.get(&picture.view).ok_or_else(|| named("no count for its view".into()))?;
        spent.set(spent.get().saturating_add(rows.pixel_tests()));
        if spent.get() > self.max_pixel_tests {
            return Err(named(over_budget(self.max_pixel_tests)));
        }
        loop {
            if let Some(pixels) = rows.band().map_err(named)? {
                return Ok(pixels);
            }
        }
    }
}

#[cfg(test)]
#[path = "drawing_picture_tests.rs"]
mod tests;
