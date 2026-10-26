//! A prepared scene on a writer's number grid (#4890, C4): the one walk every writer formats, so
//! each format draws the same joints, closures and merges.
//!
//! - Every coordinate is a whole number of the grid's steps, x to the right and y up as in the
//!   scene, so a joint is one point and a writer's flip or scale of whole numbers is exact.
//! - A closed path ends exactly on its start; a closing line is left to the writer's close
//!   (SVG's `Z`, PDF's `h`).
//! - A piece ending within [`SLIVER_STEPS`] of where the path is merges into the next, and is
//!   counted. A path's last piece has no next, so it is kept unless it has no length.
//! - A width under half a step is one step, and is counted.
//! - Circular arcs come in pieces of at most a quarter turn ([`arc_pieces`]). Ellipses, hyperbolas
//!   and Béziers above degree 3 aren't walked yet (A3b): a scene with one is refused.

use super::{Invalid, Item, ObjectKind, Prepared, Result, arc_pieces};
use crate::{Seg2, Vec2};

/// The largest number put on a grid, in millimetres either way: far past any scene's 2e5.
const MAX_NUMBER_MM: f64 = 9e9;
/// Pieces ending this many steps or fewer from where the path is are merged into the next.
pub const SLIVER_STEPS: i64 = 10;

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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GridItem {
    /// A stroke `width` steps wide, at least one.
    Stroke { width: i64, path: GridPath },
    /// A fill's loops, each closed.
    Fill { loops: Vec<GridPath> },
}

/// A group's items on the grid, in paint order.
#[derive(Clone, Debug, PartialEq, Eq)]
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
}

/// A scene on a grid: the sheet's size, its groups in paint order, and the report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OnGrid {
    pub size: GridPoint,
    pub groups: Vec<GridGroup>,
    pub report: GridReport,
}

impl Prepared<'_> {
    /// The scene on `grid`, by the rules in this module's notes: what every writer formats.
    pub fn on_grid(&self, grid: Grid) -> Result<OnGrid> {
        let mut report = GridReport::default();
        let mut groups = Vec::with_capacity(self.groups().len());
        for group in self.groups() {
            let mut items = Vec::with_capacity(group.items.len());
            for item in &group.items {
                match item {
                    Item::Stroke { path, closed, width_mm } => {
                        let Some(path) = walk(grid, path, *closed, &mut report)? else { continue };
                        let mut width = grid.steps(*width_mm)?;
                        if width == 0 {
                            width = 1;
                            report.widths_snapped += 1;
                        }
                        items.push(GridItem::Stroke { width, path });
                    }
                    Item::Fill { region } => {
                        let mut loops = Vec::new();
                        for l in region.loops() {
                            loops.extend(walk(grid, &l.segs, true, &mut report)?);
                        }
                        items.push(GridItem::Fill { loops });
                    }
                    Item::Text(_) => return Err(Invalid("not supported yet: text in an SVG or a sheet PNG (#4890 B2)")),
                }
            }
            groups.push(GridGroup { object: group.object, kind: group.kind, items });
        }
        Ok(OnGrid { size: grid.point(self.size_mm())?, groups, report })
    }
}

/// `segs` on the grid (`None` when it has no pieces).
fn walk(grid: Grid, segs: &[Seg2], closed: bool, report: &mut GridReport) -> Result<Option<GridPath>> {
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
                return Err(Invalid("not supported yet: ellipses, hyperbolas and Béziers above degree 3 on paper (#4890, A3b)"));
            }
        }
        Ok(())
    }
}
