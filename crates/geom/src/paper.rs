//! The paper scene: what a sheet shows, as strokes, fills and text in sheet millimetres, for the
//! writers that put it in a file (DXF, SVG, PNG, PDF). Plain data with one check,
//! [`Scene::validate`]. Text is runs of placed glyphs with the table of the fonts they use
//! ([`text`]). Clips, line patterns and styles are later additions.
//!
//! - Coordinates are millimetres from the sheet's lower-left corner, x to the right and y up;
//!   angles are radians, anticlockwise. A line's width is its width on paper, centred on its path.
//! - Paint goes on in order: the groups in turn, then the items of each group in turn, each over
//!   what is already there.
//! - A stroke has butt caps and miter joins with a miter limit of 4 (a join sharper than about 29°
//!   is bevelled). A closed stroke joins at its start instead of having two caps. A fill is
//!   painted even-odd, so a hole is a hole whichever way its loop runs.
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
//!
//! It does not certify simple loops, holes inside their outer loop or a path that never doubles
//! back. Validation after parsing cannot bound parsing allocations: an encoded-input boundary must
//! also limit bytes and nesting.
//!
//! Writers read a [`Prepared`] scene, which only [`Scene::prepare`] makes, so none reads one that
//! wasn't checked, and format it on their number grid ([`Prepared::on_grid`], [`grid`]'s rules),
//! so every format draws the same joints, closures and merges. Clipping will happen in
//! `prepare` too, once, before any writer (A8).

use serde::{Deserialize, Serialize};

use std::f64::consts::FRAC_PI_2;

use crate::{EPS, Region2, Seg2, Vec2};

pub mod annotate;
pub mod grid;
pub mod text;

pub use grid::{Grid, GridGroup, GridItem, GridPath, GridPoint, GridReport, OnGrid, Piece};
use text::{Font, TextRun};

/// The widest line a scene may draw, in millimetres.
pub const MAX_LINE_MM: f64 = 10.0;
/// The longest side a sheet may have, in millimetres.
pub const MAX_SIDE_MM: f64 = 5000.0;
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
            ObjectKind::View => "VIEW",
        }
    }
}

/// One mark on the paper.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Item {
    /// A line `width_mm` wide along `path`, each piece starting where the one before ends. A
    /// closed stroke also ends where it starts.
    Stroke { path: Vec<Seg2>, closed: bool, width_mm: f64 },
    /// A region painted solid.
    Fill { region: Region2 },
    /// A run of text, filled with its glyphs' outlines.
    Text(TextRun),
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

/// A scene that passed [`Scene::validate`]: what every writer reads.
#[derive(Clone, Copy, Debug)]
pub struct Prepared<'a> {
    scene: &'a Scene,
}

impl<'a> Prepared<'a> {
    pub fn size_mm(&self) -> Vec2 {
        self.scene.size_mm
    }

    /// The groups, in paint order.
    pub fn groups(&self) -> &'a [Group] {
        &self.scene.groups
    }

    /// The font a run names: there, as the scene was checked.
    pub fn font(&self, run: &TextRun) -> Option<&'a Font> {
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
        Scene { size_mm, groups, fonts: Vec::new() }
    }

    /// The scene, checked ([`Scene::validate`]), for the writers.
    pub fn prepare(&self) -> Result<Prepared<'_>> {
        self.validate()?;
        Ok(Prepared { scene: self })
    }

    /// Checks the scene against the limits in the module's notes, counting its records first.
    pub fn validate(&self) -> Result<()> {
        let mut budget = Budget(0);
        for group in &self.groups {
            budget.add(group.records())?;
        }
        let Vec2 { x, y } = self.size_mm;
        require([x, y].into_iter().all(|side| side > 0.0 && side <= MAX_SIDE_MM), "each side of the sheet is above 0 and at most 5000 mm")?;
        for item in self.groups.iter().flat_map(|g| &g.items) {
            match item {
                Item::Stroke { path, closed, width_mm } => {
                    require(*width_mm > 0.0 && *width_mm <= MAX_LINE_MM, "a line is above 0 and at most 10 mm wide")?;
                    require(!path.is_empty(), "a stroke has at least one piece")?;
                    pieces(path)?;
                    require(path.iter().zip(path.iter().skip(1)).all(|(a, b)| meet(a, b)), "a stroke's pieces meet end to start")?;
                    if *closed {
                        require(path.last().zip(path.first()).is_some_and(|(a, b)| meet(a, b)), "a closed stroke ends where it starts")?;
                    }
                }
                Item::Fill { region } => {
                    for l in region.loops() {
                        pieces(&l.segs)?;
                        require(l.is_closed(EPS), "each loop of a fill is closed")?;
                    }
                }
                Item::Text(_) => {}
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
