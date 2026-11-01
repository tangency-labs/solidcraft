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
//! their pixels for a writer: placed by the [`solidcraft_geom::drawing_view::ViewFrame`] that placed
//! the lines. Nothing is kept between exports yet: each renders again.

use std::cell::OnceCell;
use std::collections::BTreeMap;

use solidcraft_doc::drawing::Drawing;
use solidcraft_doc::ids;
use solidcraft_geom::Aabb2;
use solidcraft_geom::drawing_view::ViewFrame;
use solidcraft_geom::paper::{Item, Picture, Pictures};
use solidcraft_geom::pixel_grid::{GridWindow, PixelGrid};
use solidcraft_render::job::{BodyRole, CancelToken, OrthoFrame, PicturePoll, PictureRequest, PictureTask, PixelWindow, Projection, Supersample};
use solidcraft_render::{ProductScene, VisualStyle};

use crate::drawing_view::{ShownSet, Styles};
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

/// What placed a view's lines, as the renderer takes it.
fn ortho(frame: &ViewFrame) -> OrthoFrame {
    let basis = frame.basis();
    OrthoFrame { basis, scale: frame.scale().factor(), shift: frame.to_paper(basis.origin) }
}

struct ViewPicture {
    view: u64,
    frame: OrthoFrame,
    parts: Vec<Part>,
}

/// The pixels of the shaded views' pictures on one sheet, for the writers.
pub struct ViewPictures<'a> {
    session: &'a Session,
    /// Each picture, by its source.
    pictures: BTreeMap<u64, ViewPicture>,
    /// Every body the design places, read the first time a writer asks.
    scene: OnceCell<std::result::Result<ProductScene, String>>,
}

impl<'a> ViewPictures<'a> {
    /// The pictures of the views on sheet `sheet` of `d`, each placed as its lines are. With nothing
    /// to show, a view is a placeholder and has none.
    pub fn new(s: &'a Session, d: &Drawing, sheet: u64) -> Result<ViewPictures<'a>> {
        let mut pictures = BTreeMap::new();
        if let Ok(set) = crate::drawing_view::shown(s) {
            for (id, view, centre) in crate::drawing_scene::views_on(d, sheet)? {
                let parts = view_parts(&set, Styles::from(&view));
                let Ok(extents) = crate::drawing_view::measure(&set, &view.basis) else { continue };
                let frame = ortho(&crate::drawing_view::frame(view.basis, view.scale.value, &extents.model, centre, CMD)?);
                let count = parts.len();
                for (i, parts) in parts.into_iter().enumerate() {
                    let picture = ViewPicture { view: id, frame, parts };
                    if pictures.insert(source(id, i, count), picture).is_some() {
                        return Err(bad(CMD, format!("view {}: its picture's number is another's", ids::encode(id))));
                    }
                }
            }
        }
        Ok(ViewPictures { session: s, pictures, scene: OnceCell::new() })
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
        if whole.meshes.len() != picture.parts.len() {
            return Err(named("the model changed while it was drawn".into()));
        }
        // A body left out of a picture isn't in its scene.
        let kept = || whole.meshes.iter().zip(&picture.parts).filter(|(_, part)| **part != Part::Left);
        let meshes = kept().map(|(mesh, _)| mesh.clone()).collect();
        let roles = kept().map(|(_, part)| if *part == Part::DepthOnly { BodyRole::DepthOnly } else { BodyRole::Painted }).collect();
        let request = PictureRequest {
            projection: Projection::Ortho { frame: picture.frame, grid: *grid },
            tiles: tiles(window),
            supersample: Supersample::default(),
            roles,
            background: None,
        };
        let scene = ProductScene { meshes, bounds: whole.bounds };
        let mut task = PictureTask::new(scene, request, CancelToken::default()).map_err(|e| named(e.to_string()))?;
        let width = (window.x1 - window.x0) as usize;
        let mut pixels = vec![0; width * (window.y1 - window.y0) as usize * 4];
        loop {
            match task.step(16_384).map_err(|e| named(e.to_string()))? {
                PicturePoll::Pending(_) => {}
                PicturePoll::Tile(tile, rows) => {
                    let stride = (tile.x1 - tile.x0) as usize * 4;
                    for (row, line) in rows.chunks_exact(stride).enumerate() {
                        let at = (((tile.y0 - window.y0) as usize + row) * width + (tile.x0 - window.x0) as usize) * 4;
                        pixels[at..at + stride].copy_from_slice(line);
                    }
                }
                PicturePoll::Complete => return Ok(pixels),
            }
        }
    }
}

#[cfg(test)]
#[path = "drawing_picture_tests.rs"]
mod tests;
