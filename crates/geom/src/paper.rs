//! The paper scene: what a sheet shows, as strokes, fills and text in sheet millimetres, for the
//! writers that put it in a file (DXF, SVG, PNG, PDF). Plain data with one check,
//! [`Scene::validate`]. Text is runs of placed glyphs with the table of the fonts they use
//! ([`text`]). Clips cut an object's groups before any writer reads them ([`clip`]). Styles are a later
//! addition.
//!
//! A picture (a shaded view, a logo) is a rectangle and the name of its pixels, never the pixels:
//! a writer asks a [`Pictures`] provider for them on the pixel grid it draws on
//! ([`crate::pixel_grid`]), so a PNG at 1200 dpi gets a picture drawn at 1200 dpi rather than one
//! stretched.
//!
//! - Coordinates are millimetres from the sheet's lower-left corner, x to the right and y up;
//!   angles are radians, anticlockwise. A line's width is its width on paper, centred on its path.
//! - Paint goes on in order: the groups in turn, then the items of each group in turn, each over
//!   what is already there.
//! - A stroke has butt caps and miter joins with a miter limit of 4 (a join sharper than about 29°
//!   is bevelled). A closed stroke joins at its start instead of having two caps. A fill is
//!   painted even-odd, so a hole is a hole whichever way its loop runs.
//! - A dashed stroke's pattern ([`Dashes`]) runs along its whole path, round a closed one, in
//!   paper millimetres: a dashed line looks the same at any view scale. Each dash has the
//!   stroke's caps and joins.
//! - Each group says which object on the sheet drew it, by an id, and what kind of object that is.
//!   The id is opaque here: it names one object within the sheet, and several groups may come from
//!   the same object. DXF and PDF put a group on the layer its kind names ([`ObjectKind::layer`]).
//!
//! [`Scene::validate`] checks a scene before any writer reads it:
//! - at most [`MAX_RECORDS`] groups, items, loops and curve pieces in all, counted before any
//!   geometry is read;
//! - each side of the sheet above 0 and at most [`MAX_SIDE_MM`];
//! - each piece's numbers through the checks section geometry uses, with every stored coordinate
//!   and radius finite and at most 1e5 mm in size, so every point drawn has both coordinates
//!   within ±2e5 mm;
//! - a stroke's pieces meeting end to start within [`EPS`], and its ends too when it is closed;
//!   every loop of a fill closed the same way;
//! - a line width above 0 and at most [`MAX_LINE_MM`];
//! - the text and its fonts by [`text`]'s rules.
//! - a picture's rectangle within the coordinate bound and above 0 on each side, and its opacity
//!   above 0 and at most 1.
//! - a dash pattern of 2 to 12 lengths, an even count, each above 0 and at most [`MAX_DASH_MM`],
//!   its phase from 0 up to (not including) the pattern's length, and at most [`MAX_DASHES`]
//!   dashes along the stroke, counted from its pieces' lengths ([`Seg2::length`]).
//!
//! It does not certify simple loops, holes inside their outer loop or a path that never doubles
//! back. Validation after parsing cannot bound parsing allocations: an encoded-input boundary must
//! also limit bytes and nesting.
//!
//! Writers read a [`Prepared`] scene, which only [`Scene::prepare`] makes, so none reads one that
//! wasn't checked, and format it on their number grid ([`Prepared::on_grid`], [`grid`]'s rules),
//! so every format draws the same joints, closures and merges.
//!
//! Clips: [`Scene::prepare`] also cuts each group of an object to that object's [`Clip`], a
//! rectangle or a circle (a detail view's), once, before any writer ([`clip`]'s rules), and checks
//! what that made again, so every writer draws the same clip-free groups. A clip is checked there:
//! one an object, from its lower-left to its upper-right corner or with a radius above 0, every
//! number finite and at most 1e5 mm in size. A fill is kept or dropped whole: one crossing its
//! clip's edge isn't supported yet.

use serde::{Deserialize, Serialize};

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::f64::consts::FRAC_PI_2;

use crate::pixel_grid::{GridWindow, PixelGrid, PixelGridError};
use crate::{Aabb2, EPS, Region2, Seg2, Vec2};

pub mod annotate;
pub mod clip;
pub mod cubic_pieces;
pub mod flatten;
pub mod grid;
pub mod text;

pub use flatten::flatten;
pub use grid::{FillRule, Grid, GridDashes, GridGroup, GridItem, GridPath, GridPoint, GridReport, OnGrid, Piece};
use text::{Font, TextRun};

/// The widest line a scene may draw, in millimetres.
pub const MAX_LINE_MM: f64 = 10.0;
/// The longest dash or gap a pattern may have, in millimetres.
pub const MAX_DASH_MM: f64 = 1000.0;
/// The most dashes one stroke may need: what a writer cuts it into stays bounded.
pub const MAX_DASHES: f64 = 1e6;
/// The longest side a sheet may have, in millimetres.
pub const MAX_SIDE_MM: f64 = 5000.0;
/// The most pixels an image on paper may have: 8,000 by 8,000, 256 MB once decoded.
pub const MAX_IMAGE_PIXELS: u64 = 64_000_000;
/// Groups, items, loops and curve pieces a scene may hold in all.
pub const MAX_RECORDS: usize = 100_000;
/// The largest coordinate and radius either way, in millimetres: twenty times the longest side.
pub const MAX_COORDINATE_MM: f64 = 1e5;

/// One sheet's marks, in paint order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    /// The sheet's width and height.
    pub size_mm: Vec2,
    pub groups: Vec<Group>,
    /// The fonts the text uses, by id ([`text`]): none, and not written, without text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fonts: Vec<Font>,
    /// What objects' groups are cut to, by object: at most one each, and none when nothing is cut.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clips: Vec<(u64, Clip)>,
}

/// Items one object on the sheet drew, painted in order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    /// The id of the object that drew these items, opaque here.
    pub object: u64,
    /// What kind of object it is.
    pub kind: ObjectKind,
    pub items: Vec<Item>,
}

impl Group {
    /// The records the group counts toward [`MAX_RECORDS`]: itself, each item, each stroke's pieces,
    /// and each loop of a fill with its pieces. Saturates rather than overflows.
    pub fn records(&self) -> usize {
        let item = |item: &Item| match item {
            Item::Stroke { path, .. } => path.len(),
            Item::Fill { region } => region.loops().fold(0usize, |n, l| n.saturating_add(1).saturating_add(l.segs.len())),
            Item::Text(run) => run.records(),
            Item::Picture(_) => 0,
        };
        self.items.iter().fold(1usize, |n, i| n.saturating_add(1).saturating_add(item(i)))
    }
}

/// What kind of object drew a group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    /// The physical edge of the sheet's paper.
    SheetBorder,
    /// The outline inside the sheet border that bounds the views and annotations, with the grid
    /// and the centring and trimming marks of its profile.
    DrawingBorder,
    /// The block of fields that identifies the drawing and what its sheet documents.
    TitleBlock,
    /// The sheet table that presents an assembly's bill of materials.
    PartsList,
    /// The sheet table that lists the drawing's revisions.
    RevisionTable,
    /// An image placed on the sheet, such as a logo or a photo.
    SheetImage,
    /// A drawing view's lines.
    View,
}

impl ObjectKind {
    /// The layer DXF and PDF put this kind's groups on: its name in capitals, as DXF layers are
    /// named.
    pub fn layer(self) -> &'static str {
        match self {
            ObjectKind::SheetBorder => "SHEET_BORDER",
            ObjectKind::DrawingBorder => "DRAWING_BORDER",
            ObjectKind::TitleBlock => "TITLE_BLOCK",
            ObjectKind::PartsList => "PARTS_LIST",
            ObjectKind::RevisionTable => "REVISION_TABLE",
            ObjectKind::SheetImage => "SHEET_IMAGE",
            ObjectKind::View => "VIEW",
        }
    }
}

/// One mark on the paper.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Item {
    /// A line `width_mm` wide along `path`, each piece starting where the one before ends. A
    /// closed stroke also ends where it starts. With a `pattern` it is dashed; without, solid.
    Stroke {
        path: Vec<Seg2>,
        closed: bool,
        width_mm: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pattern: Option<Dashes>,
    },
    /// A region painted solid.
    Fill { region: Region2 },
    /// A run of text, filled with its glyphs' outlines.
    Text(TextRun),
    /// Pixels drawn into a rectangle.
    Picture(Picture),
}

/// A picture on the sheet: its pixels, named by `source`, drawn into `rect` over what is already
/// there, `opacity` times as strongly as their own alpha says.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Picture {
    /// Which pixels, opaque here: a writer asks [`Pictures`] for them.
    pub source: u64,
    /// Where it sits, millimetres.
    pub rect: Aabb2,
    /// Above 0, up to 1 (as its pixels are).
    pub opacity: f64,
}

impl Picture {
    /// Its own pixel grid at `dpi`, anchored at its rectangle's lower-left corner: what SVG and PDF
    /// embed.
    pub fn grid(&self, dpi: f64) -> std::result::Result<PixelGrid, PixelGridError> {
        let size = self.rect.size();
        PixelGrid::new(self.rect.min.x, self.rect.min.y, size.x, size.y, dpi)
    }
}

/// Where a writer gets a picture's pixels.
pub trait Pictures {
    /// The pixels of picture `source` on `grid`, for the columns and rows of `window`: straight
    /// (not premultiplied) alpha, sRGB, 8 bits a channel, RGBA, top row first, exactly `window`'s
    /// size.
    fn rows(&self, source: u64, grid: &PixelGrid, window: GridWindow) -> std::result::Result<Vec<u8>, String>;
}

/// No pictures: a writer given it refuses any picture it meets.
pub struct NoPictures;

impl Pictures for NoPictures {
    fn rows(&self, source: u64, _: &PixelGrid, _: GridWindow) -> std::result::Result<Vec<u8>, String> {
        Err(format!("no pixels for picture {source}"))
    }
}

/// What one object's groups are cut to: only what lies inside is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Clip {
    /// The rectangle from its lower-left corner `min` to its upper-right corner `max`.
    Rect { min: Vec2, max: Vec2 },
    /// The disc of `radius` round `center`.
    Circle { center: Vec2, radius: f64 },
}

impl Clip {
    /// Checks the clip against the module's notes.
    fn check(&self) -> Result<()> {
        let within = |v: f64| v.is_finite() && v.abs() <= MAX_COORDINATE_MM;
        let fine = match *self {
            Clip::Rect { min, max } => [min.x, min.y, max.x, max.y].into_iter().all(within) && min.x < max.x && min.y < max.y,
            Clip::Circle { center, radius } => within(center.x) && within(center.y) && radius > 0.0 && radius <= MAX_COORDINATE_MM,
        };
        require(fine, "a clip is a rectangle from its lower-left to its upper-right corner, or a circle with a radius above 0, within 1e5 mm")
    }
}

/// A dashed line's pattern: dash and gap lengths in turn, starting with a dash, and how far into
/// the pattern the stroke starts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dashes {
    pub lengths_mm: Vec<f64>,
    pub phase_mm: f64,
}

impl Dashes {
    /// Checks the pattern, and the dashes it needs along `path`, against the module's notes.
    fn check(&self, path: &[Seg2]) -> Result<()> {
        let n = self.lengths_mm.len();
        require((2..=12).contains(&n) && n.is_multiple_of(2), "a dash pattern has an even count of 2 to 12 lengths")?;
        require(self.lengths_mm.iter().all(|l| *l > 0.0 && *l <= MAX_DASH_MM), "each dash and gap is above 0 and at most 1000 mm")?;
        let period: f64 = self.lengths_mm.iter().sum();
        require(self.phase_mm >= 0.0 && self.phase_mm < period, "a dash pattern's phase is from 0 up to its length")?;
        // Counted as writers that cut a line into its dashes count them: pairs along the length.
        let length: f64 = path.iter().map(Seg2::length).sum();
        require(length / period * (n / 2) as f64 <= MAX_DASHES, "a dashed line needs at most 1,000,000 dashes")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("paper scene: {0}")]
pub struct Invalid(pub &'static str);

type Result<T> = std::result::Result<T, Invalid>;

fn require(condition: bool, message: &'static str) -> Result<()> {
    if condition { Ok(()) } else { Err(Invalid(message)) }
}

struct Budget(usize);
impl Budget {
    fn add(&mut self, n: usize) -> Result<()> {
        const MESSAGE: &str = "more than 100,000 groups, items, loops and pieces";
        self.0 = self.0.checked_add(n).ok_or(Invalid(MESSAGE))?;
        require(self.0 <= MAX_RECORDS, MESSAGE)
    }
}

fn pieces(segs: &[Seg2]) -> Result<()> {
    segs.iter().try_for_each(|s| s.check(MAX_COORDINATE_MM).map_err(Invalid))
}

/// Whether `b` starts where `a` ends.
fn meet(a: &Seg2, b: &Seg2) -> bool {
    a.end().dist(b.start()) <= EPS
}

/// A scene that passed [`Scene::validate`], its strokes and fills cut to its clips: what every
/// writer reads. Without clips the scene is borrowed as it is; with them it is the scene cut to them.
#[derive(Clone, Debug)]
pub struct Prepared<'a> {
    scene: Cow<'a, Scene>,
    /// Each clipped object's clip: pixels aren't cut until a writer draws them.
    clips: BTreeMap<u64, Clip>,
}

impl Prepared<'_> {
    pub fn size_mm(&self) -> Vec2 {
        self.scene.size_mm
    }

    /// The groups, in paint order.
    pub fn groups(&self) -> &[Group] {
        &self.scene.groups
    }

    /// The font a run names: there, as the scene was checked.
    pub fn font(&self, run: &TextRun) -> Option<&Font> {
        let fonts = &self.scene.fonts;
        fonts.binary_search_by_key(&run.font, |f| f.id).ok().and_then(|at| fonts.get(at))
    }
}

/// The ends of the pieces of at most a quarter turn the arc of `sweep` radians from `start`
/// round `center` is drawn in, in order, the last its own end. For writers that draw an arc from
/// its ends (SVG's `A`): there a half turn's centre is ill-conditioned, and moves by up to 1e-2 mm
/// once its ends are rounded to the nanometre (six decimals of a millimetre), where a quarter
/// turn's stays under 1e-6 mm.
pub fn arc_pieces(center: Vec2, radius: f64, start: f64, sweep: f64) -> impl Iterator<Item = Vec2> {
    // A scene's sweeps are at most a whole turn and a little: one to four pieces.
    let n = ((sweep.abs() / FRAC_PI_2) - 1e-9).ceil().clamp(1.0, 4.0) as usize;
    (1..=n).map(move |k| {
        let angle = if k == n { start + sweep } else { start + sweep * (k as f64 / n as f64) };
        center + Vec2::from_angle(angle) * radius
    })
}

impl Scene {
    /// A scene of `groups` on a sheet `size_mm`, with no text yet.
    pub fn new(size_mm: Vec2, groups: Vec<Group>) -> Scene {
        Scene { size_mm, groups, fonts: Vec::new(), clips: Vec::new() }
    }

    /// The scene, checked ([`Scene::validate`]), for the writers, with each group of a clipped object
    /// cut to its clip. What the cuts made is checked again: a cut adds pieces, and the scene may then
    /// hold too many.
    pub fn prepare(&self) -> Result<Prepared<'_>> {
        self.validate()?;
        if self.clips.is_empty() {
            return Ok(Prepared { scene: Cow::Borrowed(self), clips: BTreeMap::new() });
        }
        let clips: BTreeMap<u64, Clip> = self.clips.iter().copied().collect();
        let mut groups = Vec::with_capacity(self.groups.len());
        for group in &self.groups {
            match clips.get(&group.object) {
                Some(clip) => groups.push(clip::clipped(group, clip)?),
                None => groups.push(group.clone()),
            }
        }
        let cut = Scene { size_mm: self.size_mm, groups, fonts: self.fonts.clone(), clips: Vec::new() };
        cut.validate()?;
        Ok(Prepared { scene: Cow::Owned(cut), clips })
    }

    /// Checks the clips against the module's notes: few enough, one for each object, each well made.
    fn check_clips(&self) -> Result<()> {
        require(self.clips.len() <= MAX_RECORDS, "more than 100,000 clips")?;
        let mut objects: Vec<u64> = self.clips.iter().map(|(object, _)| *object).collect();
        objects.sort_unstable();
        require(objects.windows(2).all(|w| w[0] != w[1]), "an object has one clip")?;
        self.clips.iter().try_for_each(|(_, clip)| clip.check())
    }

    /// Checks the scene against the limits in the module's notes, counting its records first.
    pub fn validate(&self) -> Result<()> {
        self.check_clips()?;
        let mut budget = Budget(0);
        for group in &self.groups {
            budget.add(group.records())?;
        }
        let Vec2 { x, y } = self.size_mm;
        require([x, y].into_iter().all(|side| side > 0.0 && side <= MAX_SIDE_MM), "each side of the sheet is above 0 and at most 5000 mm")?;
        for item in self.groups.iter().flat_map(|g| &g.items) {
            match item {
                Item::Stroke { path, closed, width_mm, pattern } => {
                    require(*width_mm > 0.0 && *width_mm <= MAX_LINE_MM, "a line is above 0 and at most 10 mm wide")?;
                    require(!path.is_empty(), "a stroke has at least one piece")?;
                    pieces(path)?;
                    require(path.iter().zip(path.iter().skip(1)).all(|(a, b)| meet(a, b)), "a stroke's pieces meet end to start")?;
                    if *closed {
                        require(path.last().zip(path.first()).is_some_and(|(a, b)| meet(a, b)), "a closed stroke ends where it starts")?;
                    }
                    if let Some(dashes) = pattern {
                        dashes.check(path)?;
                    }
                }
                Item::Fill { region } => {
                    for l in region.loops() {
                        pieces(&l.segs)?;
                        require(l.is_closed(EPS), "each loop of a fill is closed")?;
                    }
                }
                Item::Text(_) => {}
                Item::Picture(Picture { rect, opacity, .. }) => {
                    let within = |v: f64| v.abs() <= MAX_COORDINATE_MM;
                    let corners = [rect.min.x, rect.min.y, rect.max.x, rect.max.y];
                    require(
                        corners.into_iter().all(within) && rect.min.x < rect.max.x && rect.min.y < rect.max.y,
                        "a picture's rectangle is within 1e5 mm and above 0 on each side",
                    )?;
                    require(*opacity > 0.0 && *opacity <= 1.0, "a picture's opacity is above 0 and at most 1")?;
                }
            }
        }
        let runs = self.groups.iter().flat_map(|g| &g.items).filter_map(|item| match item {
            Item::Text(run) => Some(run),
            _ => None,
        });
        text::check(&self.fonts, runs, &mut budget)
    }
}

#[cfg(test)]
#[path = "paper_tests.rs"]
mod tests;
