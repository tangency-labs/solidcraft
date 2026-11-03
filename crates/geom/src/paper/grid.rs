//! A prepared scene on a writer's number grid (#4890, C4): the one walk every writer formats, so
//! each format draws the same joints, closures and merges.
//!
//! - Every coordinate is a whole number of the grid's steps, x to the right and y up as in the
//!   scene, so a joint is one point and a writer's flip or scale of whole numbers is exact.
//! - A closed path ends exactly on its start; a closing line is left to the writer's close
//!   (SVG's `Z`, PDF's `h`).
//! - A piece ending within [`SLIVER_STEPS`] of where the path is merges into the next, and is
//!   counted. A path's last piece has no next, so it is kept unless it has no length.
//! - A width, dash or gap under half a step is one step, and is counted. A dash pattern's phase is
//!   taken within the pattern's length on the grid.
//! - Circular arcs come in pieces of at most a quarter turn ([`arc_pieces`]). Ellipses, hyperbolas
//!   and Béziers above degree 3 come as cubics within [`CURVE_ERROR_MM`] ([`cubic_pieces`]), at most
//!   [`MAX_CURVE_CUBICS`] in a scene, and are counted.
//! - A text run is its glyphs' outlines as the run places them
//!   ([`super::text::TextRun::outlines`]), each loop walked as a fill's, filled nonzero (B2). It
//!   keeps its item's place in the group, so a writer that sets real text (PDF) finds the run in
//!   paint order.
//! - A scene makes at most [`MAX_GRID_PIECES`] pieces on the grid ([`place`]). A run's glyphs are
//!   counted once in the scene's records, for a writer that embeds its font, but each placed glyph
//!   is its whole outline here.

use super::cubic_pieces::cubic_pieces;
use super::{Clip, Invalid, Item, ObjectKind, Picture, Prepared, Result, arc_pieces, require};
use crate::{Aabb2, Seg2, Vec2};

/// The largest number put on a grid, in millimetres either way: far past any scene's 2e5.
const MAX_NUMBER_MM: f64 = 9e9;
/// Pieces ending this many steps or fewer from where the path is are merged into the next.
pub const SLIVER_STEPS: i64 = 10;
/// How far a conic's or a higher Bézier's cubics may be from it before rounding, in millimetres.
pub const CURVE_ERROR_MM: f64 = 1e-4;
/// The most cubics a scene's conics and higher Béziers may become.
pub const MAX_CURVE_CUBICS: usize = 1_000_000;
/// The most pieces a scene makes on a grid: 56 bytes each, so about 235 MB. It is past what a sheet
/// dense with text makes: measured, a Latin letter is 15 pieces on average and a Japanese one 74
/// (171 at most), so a sheet of 20,000 Japanese letters makes about 1.5 million.
pub const MAX_GRID_PIECES: usize = 1 << 22;

/// Counts `n` more pieces into `placed`, refused past [`MAX_GRID_PIECES`]: for every writer that
/// places outlines (the walk here, a PDF that draws its text as outlines).
pub fn place(placed: &mut usize, n: usize) -> Result<()> {
    *placed = placed.saturating_add(n);
    if *placed <= MAX_GRID_PIECES {
        Ok(())
    } else {
        Err(Invalid(
            "a sheet's lines, fills and letters are more than 4,194,304 pieces on paper: put some of them (its text, most likely) on another sheet",
        ))
    }
}

/// A writer's number grid: `per_mm` steps a millimetre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    per_mm: f64,
}

impl Grid {
    /// Whole nanometres, six decimals of a millimetre: SVG's numbers.
    pub const NANOMETRE: Grid = Grid { per_mm: 1e6 };

    /// A grid of `per_mm` steps a millimetre, from 1 to 1e9. A power of two keeps every number
    /// exact in binary floating point, as a writer of 32-bit numbers (PDF) wants.
    pub fn new(per_mm: f64) -> Option<Grid> {
        (1.0..=1e9).contains(&per_mm).then_some(Grid { per_mm })
    }

    pub fn per_mm(self) -> f64 {
        self.per_mm
    }

    /// `v` millimetres as whole steps.
    pub fn steps(self, v: f64) -> Result<i64> {
        let n = (v * self.per_mm).round();
        if n.abs() <= MAX_NUMBER_MM * self.per_mm { Ok(n as i64) } else { Err(Invalid("a number too large (or not a number) for the grid")) }
    }

    pub fn point(self, p: Vec2) -> Result<GridPoint> {
        Ok(GridPoint { x: self.steps(p.x)?, y: self.steps(p.y)? })
    }
}

/// A point on a grid, in whole steps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GridPoint {
    pub x: i64,
    pub y: i64,
}

/// One piece of a path on the grid, from where the path is to `to`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Piece {
    Line {
        to: GridPoint,
    },
    /// A circular arc of at most a quarter turn, of `radius` steps, turning clockwise (y up) when
    /// `clockwise`.
    Arc {
        radius: i64,
        clockwise: bool,
        to: GridPoint,
    },
    Cubic {
        c1: GridPoint,
        c2: GridPoint,
        to: GridPoint,
    },
    /// A parabola: a conic of weight 1 with its control point `c`.
    Quadratic {
        c: GridPoint,
        to: GridPoint,
    },
}

impl Piece {
    pub fn to(&self) -> GridPoint {
        match *self {
            Piece::Line { to } | Piece::Arc { to, .. } | Piece::Cubic { to, .. } | Piece::Quadratic { to, .. } => to,
        }
    }
}

/// A path on the grid: its pieces from `start`. A closed one ends on `start`, its closing line
/// left to the writer's close.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridPath {
    pub start: GridPoint,
    pub pieces: Vec<Piece>,
    pub closed: bool,
}

/// An item on the grid.
#[derive(Clone, Debug, PartialEq)]
pub enum GridItem {
    /// A stroke `width` steps wide, at least one, dashed when it has `dashes`.
    Stroke { width: i64, path: GridPath, dashes: Option<GridDashes> },
    /// A fill's loops, each closed, filled even-odd.
    Fill { loops: Vec<GridPath> },
    /// A picture as the scene has it: its pixels are placed by [`crate::pixel_grid`], not this grid.
    /// When it crosses its object's clip, `clip` is what the writer draws it inside. `visible` is
    /// what of it can show, the same for every writer: its rectangle on the sheet, inside its clip.
    Picture { picture: Picture, clip: Option<Clip>, visible: Aabb2 },
    /// A text run's glyph outlines, each loop closed, filled nonzero; `item` is the run's index
    /// among its group's items.
    Text { item: usize, loops: Vec<GridPath> },
}

/// Which parts of a fill's loops are inside: a text run fills as a fill does, by another rule, so
/// no writer tells the two apart but one that sets real text (PDF).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillRule {
    /// Where the loops are crossed an odd number of times: the scene's fills, whose holes are loops.
    EvenOdd,
    /// Where they wind round at all: glyph outlines, whose contours may overlap.
    NonZero,
}

/// A dash pattern on the grid: dash and gap lengths in turn, each at least one step, and the
/// phase, from 0 up to the pattern's length.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridDashes {
    pub lengths: Vec<i64>,
    pub phase: i64,
}

/// A group's items on the grid, in paint order.
#[derive(Clone, Debug, PartialEq)]
pub struct GridGroup {
    pub object: u64,
    pub kind: ObjectKind,
    pub items: Vec<GridItem>,
}

/// What the grid made of a scene that the scene didn't say.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GridReport {
    /// Widths under half a step, made one step.
    pub widths_snapped: usize,
    /// Pieces too short for the grid, merged into the next.
    pub pieces_merged: usize,
    /// Dashes and gaps under half a step, made one step.
    pub dashes_snapped: usize,
    /// Cubics made from ellipses, hyperbolas and Béziers above degree 3.
    pub curve_cubics: usize,
}

/// A scene on a grid: the sheet's size, its groups in paint order, and the report.
#[derive(Clone, Debug, PartialEq)]
pub struct OnGrid {
    pub size: GridPoint,
    pub groups: Vec<GridGroup>,
    pub report: GridReport,
}

impl Prepared<'_> {
    /// The scene on `grid`, by the rules in this module's notes: what every writer formats.
    pub fn on_grid(&self, grid: Grid) -> Result<OnGrid> {
        let mut report = GridReport::default();
        let sheet = Aabb2 { min: Vec2::new(0.0, 0.0), max: self.size_mm() };
        let mut placed = 0;
        let mut groups = Vec::with_capacity(self.groups().len());
        for group in self.groups() {
            let mut items = Vec::with_capacity(group.items.len());
            for (at, item) in group.items.iter().enumerate() {
                match item {
                    Item::Stroke { path, closed, width_mm, pattern } => {
                        let Some(path) = walk(grid, path, *closed, &mut report, &mut placed)? else { continue };
                        let width = at_least_one(grid.steps(*width_mm)?, &mut report.widths_snapped);
                        let dashes = match pattern {
                            Some(d) => {
                                let mut lengths = Vec::with_capacity(d.lengths_mm.len());
                                for l in &d.lengths_mm {
                                    lengths.push(at_least_one(grid.steps(*l)?, &mut report.dashes_snapped));
                                }
                                // Rounded on its own, the phase can reach the rounded pattern's length.
                                let phase = grid.steps(d.phase_mm)? % lengths.iter().sum::<i64>();
                                Some(GridDashes { lengths, phase })
                            }
                            None => None,
                        };
                        items.push(GridItem::Stroke { width, path, dashes });
                    }
                    Item::Fill { region } => {
                        let mut loops = Vec::new();
                        for l in region.loops() {
                            loops.extend(walk(grid, &l.segs, true, &mut report, &mut placed)?);
                        }
                        items.push(GridItem::Fill { loops });
                    }
                    Item::Picture(picture) => {
                        let clip = self.clips.get(&group.object).filter(|clip| !clip.holds_all(&picture.rect)).copied();
                        let shown = clip.map_or(sheet, |clip| sheet.intersection(&clip.bounds()));
                        items.push(GridItem::Picture { picture: *picture, clip, visible: picture.rect.intersection(&shown) });
                    }
                    Item::Text(run) => {
                        // `validate` holds every run's font in the scene.
                        let font = self.font(run).ok_or(Invalid("a run's font is in the scene's table"))?;
                        let mut loops = Vec::new();
                        for l in run.outlines(font) {
                            loops.extend(walk(grid, &l.segs, true, &mut report, &mut placed)?);
                        }
                        items.push(GridItem::Text { item: at, loops });
                    }
                }
            }
            groups.push(GridGroup { object: group.object, kind: group.kind, items });
        }
        Ok(OnGrid { size: grid.point(self.size_mm())?, groups, report })
    }
}

/// `steps`, or one step when it rounds to none, counted in `snapped`.
fn at_least_one(steps: i64, snapped: &mut usize) -> i64 {
    if steps == 0 {
        *snapped += 1;
        return 1;
    }
    steps
}

/// `segs` on the grid (`None` when it has no pieces), its pieces counted into `placed` ([`place`]).
fn walk(grid: Grid, segs: &[Seg2], closed: bool, report: &mut GridReport, placed: &mut usize) -> Result<Option<GridPath>> {
    let (Some(first), Some((last, body))) = (segs.first(), segs.split_last()) else { return Ok(None) };
    let start = grid.point(first.start())?;
    let mut w = Walk { grid, start, at: start, pieces: Vec::with_capacity(segs.len()), report };
    for seg in body {
        w.piece(seg, false, false)?;
    }
    if !closed {
        w.piece(last, false, true)?;
    } else if !matches!(last, Seg2::Line { .. }) {
        w.piece(last, true, true)?;
    }
    place(placed, w.pieces.len())?;
    Ok(Some(GridPath { start, pieces: w.pieces, closed }))
}

/// A path being walked: where it started and where it is.
struct Walk<'r> {
    grid: Grid,
    start: GridPoint,
    at: GridPoint,
    pieces: Vec<Piece>,
    report: &'r mut GridReport,
}

impl Walk<'_> {
    /// Whether a piece ending at `end` is too short to keep: it merges into the next. A path's
    /// last piece (`keep`) has no next, so it is kept unless it has no length: an open path ends
    /// where it ends, a closed one on its start exactly.
    fn merged(&mut self, end: GridPoint, keep: bool) -> bool {
        if keep {
            return end == self.at;
        }
        let (dx, dy) = (end.x - self.at.x, end.y - self.at.y);
        let short = dx.abs() <= SLIVER_STEPS && dy.abs() <= SLIVER_STEPS && dx * dx + dy * dy <= SLIVER_STEPS * SLIVER_STEPS;
        if short {
            self.report.pieces_merged += 1;
        }
        short
    }

    fn push(&mut self, piece: Piece) {
        self.at = piece.to();
        self.pieces.push(piece);
    }

    /// One piece, from where the path is: `closing` makes its end the path's start, and `keep`
    /// marks the path's last piece ([`Walk::merged`]).
    fn piece(&mut self, seg: &Seg2, closing: bool, keep: bool) -> Result<()> {
        let end = if closing { self.start } else { self.grid.point(seg.end())? };
        match *seg {
            Seg2::Line { .. } => {
                if !self.merged(end, keep) {
                    self.push(Piece::Line { to: end });
                }
            }
            Seg2::Arc { center, radius, start, sweep } => {
                let (radius_steps, clockwise) = (self.grid.steps(radius)?, sweep < 0.0);
                let mut ends = arc_pieces(center, radius, start, sweep).peekable();
                while let Some(p) = ends.next() {
                    let final_piece = ends.peek().is_none();
                    let to = if final_piece { end } else { self.grid.point(p)? };
                    if !self.merged(to, keep && final_piece) {
                        self.push(Piece::Arc { radius: radius_steps, clockwise, to });
                    }
                }
            }
            Seg2::Cubic { p1, p2, .. } => {
                let (c1, c2) = (self.grid.point(p1)?, self.grid.point(p2)?);
                self.push(Piece::Cubic { c1, c2, to: end });
            }
            Seg2::Conic { apex, w, .. } if (w - 1.0).abs() <= 1e-12 => {
                let c = self.grid.point(apex)?;
                self.push(Piece::Quadratic { c, to: end });
            }
            Seg2::Conic { .. } | Seg2::Bezier { .. } => {
                let (cubics, _) = cubic_pieces(seg, CURVE_ERROR_MM)?;
                self.report.curve_cubics = self.report.curve_cubics.saturating_add(cubics.len());
                require(self.report.curve_cubics <= MAX_CURVE_CUBICS, "a scene's curves need more than 1,000,000 cubics on paper")?;
                let last = cubics.len().saturating_sub(1);
                for (i, cubic) in cubics.iter().enumerate() {
                    let Seg2::Cubic { p1, p2, p3, .. } = *cubic else { continue };
                    let to = if i == last { end } else { self.grid.point(p3)? };
                    self.push(Piece::Cubic { c1: self.grid.point(p1)?, c2: self.grid.point(p2)?, to });
                }
            }
        }
        Ok(())
    }
}
