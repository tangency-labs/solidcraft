//! The holes of a perforation field on a sheet made from surfaces (#732, #6748): their shape
//! (round, hexagonal or slotted), the room they leave between them and to the sheet's edges, and
//! the grid that lays them out by their own outlines. A field with no shape or row pitch of its
//! own is round holes on the pattern's rows, laid out by [`crate::sheet::perforation_grid`].

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use serde::{Deserialize, Serialize};
use solidcore_record::{Loop2, Region2, Seg2, Vec2};
use solidcraft_geom::prelude::*;
use solidcraft_geom::{Aabb2, LINEAR_TOL};

use crate::sheet::SheetPerforation;
use crate::{DocError, Result};

/// The most places for holes a shaped field's grid steps through (its rows times the places along
/// a row, over the field's box): a work limit, so a finer field is refused rather than laid out
/// for ever.
const MAX_SITES: f64 = 4_000_000.0;

/// The most cells along each side of the grid that buckets the edges near each hole.
const MAX_CELLS: f64 = 1024.0;

/// The most entries that grid holds, an edge counted once in each cell near it: a work and
/// memory limit, so a sheet whose edges are too many and too long for the field's grid is
/// refused rather than bucketed without end.
const MAX_ENTRIES: usize = 1 << 24;

/// The most pieces a line or an arc is boxed in for that grid.
const MAX_PIECES: f64 = 4096.0;

/// How far inside its margin a hole may come and still be kept: a rounding, not room. The grid
/// keeps a hole this close to an edge it keeps clear of, a bend zone's among them, so a hole may
/// reach this far into such a zone and still lie on its panel (see
/// [`crate::sheet_surface_perforation`]). The round holes' grid, [`crate::sheet::perforation_grid`],
/// keeps the same 1e-9.
pub(crate) const GRAZE: f64 = 1e-9;

/// The chords a curve's nearest points are first looked for on, within this (mm) of the curve;
/// Newton then finds them on the curve itself.
const CHORD: f64 = 1e-3;

/// The shape of a perforation field's holes. They lie along the grid's rows (the flat's X).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerforationShape {
    /// Round, `diameter` across.
    #[default]
    Round,
    /// A regular hexagon `diameter` across its flats, with a corner up: on a staggered grid each
    /// flat faces a neighbour, a honeycomb with bars of even width.
    Hex,
    /// A slot `diameter` wide and `length` long overall, with round ends.
    Slot { length: f64 },
}

impl PerforationShape {
    pub fn is_round(&self) -> bool {
        *self == PerforationShape::Round
    }
}

/// A perforation field with the shape of its holes and the distance between its rows (None: the
/// pattern's own).
#[derive(Clone, Copy, Debug)]
pub struct ShapedPerforation<'a> {
    pub field: &'a SheetPerforation,
    pub shape: PerforationShape,
    pub row_pitch: Option<f64>,
}

/// A perforation field's figures (see [`crate::SurfaceSheet::perforation_stats`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PerforationStats {
    pub holes: usize,
    /// The holes' area (mm²).
    pub open_area: f64,
    /// The field's area (mm²): the outline on the sheet's material, else the whole flat. None
    /// where the outline can't be laid on the material (a boolean past its limits).
    pub field_area: Option<f64>,
    /// The open area over the field's, where that is known.
    pub open_share: Option<f64>,
}

impl ShapedPerforation<'_> {
    /// Round holes on the pattern's own rows: the field as it was before holes had shapes.
    pub fn is_plain(&self) -> bool {
        self.shape.is_round() && self.row_pitch.is_none()
    }

    /// The distance between rows: `row_pitch`, else the pitch's 60° rows (staggered) or the pitch (square).
    pub fn row_spacing(&self) -> f64 {
        let p = self.field;
        self.row_pitch.unwrap_or(if p.square { p.pitch } else { p.pitch * 3f64.sqrt() / 2.0 })
    }

    /// How far a hole reaches from its centre.
    pub fn reach(&self) -> f64 {
        let d = self.field.diameter;
        match self.shape {
            PerforationShape::Round => d / 2.0,
            PerforationShape::Hex => d / 3f64.sqrt(),
            PerforationShape::Slot { length } => length / 2.0,
        }
    }

    /// Half a hole's width (along the rows) and height.
    pub fn half_extent(&self) -> Vec2 {
        let r = self.field.diameter / 2.0;
        match self.shape {
            PerforationShape::Round => Vec2::new(r, r),
            PerforationShape::Hex => Vec2::new(r, self.field.diameter / 3f64.sqrt()),
            PerforationShape::Slot { length } => Vec2::new(length / 2.0, r),
        }
    }

    /// How far a hole reaches from its centre along the unit vector `n`.
    pub(crate) fn support(&self, n: Vec2) -> f64 {
        let r = self.field.diameter / 2.0;
        match self.shape {
            PerforationShape::Round => r,
            PerforationShape::Hex => self.hex_corners(Vec2::default()).iter().map(|k| k.dot(n)).fold(f64::MIN, f64::max),
            PerforationShape::Slot { length } => (length - self.field.diameter) / 2.0 * n.x.abs() + r,
        }
    }

    /// One hole's area.
    pub fn hole_area(&self) -> f64 {
        let d = self.field.diameter;
        match self.shape {
            PerforationShape::Round => PI * d * d / 4.0,
            PerforationShape::Hex => 3f64.sqrt() / 2.0 * d * d,
            PerforationShape::Slot { length } => d * (length - d) + PI * d * d / 4.0,
        }
    }

    /// One hole's perimeter: its wall's length round the hole.
    pub fn hole_perimeter(&self) -> f64 {
        let d = self.field.diameter;
        match self.shape {
            PerforationShape::Round => PI * d,
            PerforationShape::Hex => 2.0 * 3f64.sqrt() * d,
            PerforationShape::Slot { length } => 2.0 * (length - d) + PI * d,
        }
    }

    /// One hole's second moments of area about its centre: along the rows (∫x² dA) and across
    /// them (∫y² dA).
    pub fn hole_second_moments(&self) -> (f64, f64) {
        let d = self.field.diameter;
        let r = d / 2.0;
        match self.shape {
            PerforationShape::Round => (PI * r.powi(4) / 4.0, PI * r.powi(4) / 4.0),
            // A regular hexagon's are the same about every line through its centre: 5√3/16 of
            // its side to the fourth.
            PerforationShape::Hex => {
                let i = 5.0 * 3f64.sqrt() / 16.0 * (d / 3f64.sqrt()).powi(4);
                (i, i)
            }
            // The rectangle between the round ends, and the two half discs `s` apart.
            PerforationShape::Slot { length } => {
                let s = length - d;
                let along = d * s.powi(3) / 12.0 + PI * r * r * s * s / 4.0 + 4.0 / 3.0 * s * r.powi(3) + PI * r.powi(4) / 4.0;
                (along, s * d.powi(3) / 12.0 + PI * r.powi(4) / 4.0)
            }
        }
    }

    /// The area `n` holes open. Round holes are reckoned as they always were.
    pub fn open_area(&self, n: usize) -> f64 {
        match self.shape {
            PerforationShape::Round => n as f64 * PI * (self.field.diameter / 2.0).powi(2),
            _ => n as f64 * self.hole_area(),
        }
    }

    /// The grid's open share: one hole's area over the area each hole has (pitch × row spacing),
    /// the figure perforated sheet is sold by.
    pub fn nominal_open_share(&self) -> f64 {
        self.hole_area() / (self.field.pitch * self.row_spacing())
    }

    /// The corners of a hexagonal hole round `c`, counter-clockwise from the one at 30°.
    fn hex_corners(&self, c: Vec2) -> [Vec2; 6] {
        self.hex_corners_in(c, 0.0)
    }

    /// The corners of the hexagon `inset` in from every side of the hexagonal hole round `c`.
    fn hex_corners_in(&self, c: Vec2, inset: f64) -> [Vec2; 6] {
        let r = (self.field.diameter - 2.0 * inset) / 3f64.sqrt();
        std::array::from_fn(|k| c + Vec2::from_angle(PI / 6.0 + k as f64 * PI / 3.0) * r)
    }

    /// The straight part of a slot round `c`: its round ends' centres.
    fn slot_core(&self, c: Vec2, length: f64) -> (Vec2, Vec2) {
        let h = (length - self.field.diameter) / 2.0;
        (c - Vec2::new(h, 0.0), c + Vec2::new(h, 0.0))
    }

    /// The hole round `c`, counter-clockwise. A round hole is two half circles, as a cut-out's.
    pub fn hole_loop(&self, c: Vec2) -> Loop2 {
        let r = self.field.diameter / 2.0;
        match self.shape {
            PerforationShape::Round => Loop2 {
                segs: vec![Seg2::Arc { center: c, radius: r, start: 0.0, sweep: PI }, Seg2::Arc { center: c, radius: r, start: PI, sweep: PI }],
            },
            PerforationShape::Hex => Loop2::polygon(&self.hex_corners(c)),
            PerforationShape::Slot { length } => {
                let (a, b) = self.slot_core(c, length);
                Loop2 {
                    segs: vec![
                        Seg2::Line { a: a - Vec2::new(0.0, r), b: b - Vec2::new(0.0, r) },
                        Seg2::Arc { center: b, radius: r, start: -FRAC_PI_2, sweep: PI },
                        Seg2::Line { a: b + Vec2::new(0.0, r), b: a + Vec2::new(0.0, r) },
                        Seg2::Arc { center: a, radius: r, start: FRAC_PI_2, sweep: PI },
                    ],
                }
            }
        }
    }

    /// How far the hole round `c` stays clear of the edge `e`: 0 where the edge touches the hole,
    /// less where it reaches into it. An edge that comes more than [`GRAZE`] into a hexagon (to
    /// within GRAZE of the hexagon 2 GRAZE in from every side) reads as at least 2 GRAZE in, or as
    /// far as it reaches into the hexagon's inscribed circle; one that comes less, as touching it.
    fn clearance(&self, c: Vec2, e: &Edge) -> f64 {
        let r = self.field.diameter / 2.0;
        match self.shape {
            PerforationShape::Round => e.distance(c) - r,
            PerforationShape::Slot { length } => {
                let (p, q) = self.slot_core(c, length);
                e.distance_to_segment(p, q) - r
            }
            PerforationShape::Hex => {
                let gap = self.hex_gap(c, 0.0, e);
                if gap > 0.0 {
                    gap
                } else if self.hex_gap(c, 2.0 * GRAZE, e) > GRAZE {
                    0.0
                } else {
                    (e.distance(c) - r).min(-2.0 * GRAZE)
                }
            }
        }
    }

    /// How far the edge `e` stays clear of the hexagon `inset` in from every side of the
    /// hexagonal hole round `c`: to its nearest side, 0 where an end of the edge lies inside it or
    /// the edge meets a side.
    fn hex_gap(&self, c: Vec2, inset: f64, e: &Edge) -> f64 {
        let k = self.hex_corners_in(c, inset);
        let sides: [(Vec2, Vec2); 6] = std::array::from_fn(|i| (k[i], k[(i + 1) % 6]));
        let inside = |p: Vec2| sides.iter().all(|&(a, b)| (b - a).cross(p - a) >= 0.0);
        if inside(e.start()) || inside(e.end()) {
            return 0.0;
        }
        sides.iter().map(|&(a, b)| e.distance_to_segment(a, b)).fold(f64::MAX, f64::min)
    }

    /// Whether the hole round `c`, less [`GRAZE`] all round, and `region` overlap: a loop of the
    /// region reaches into it, measured as the grid measures an edge (see [`Edge`]); else the hole
    /// lies on one side of every loop, the side its centre is on, read from the curves themselves.
    pub(crate) fn overlaps(&self, c: Vec2, region: &Region2) -> bool {
        // The same hole, GRAZE in from every side.
        let p = self.field;
        let less = SheetPerforation {
            feature: p.feature,
            diameter: p.diameter - 2.0 * GRAZE,
            pitch: p.pitch,
            margin: p.margin,
            square: p.square,
            from_right: p.from_right,
            outline: None,
        };
        let shape = match self.shape {
            PerforationShape::Slot { length } => PerforationShape::Slot { length: length - 2.0 * GRAZE },
            other => other,
        };
        let hole = ShapedPerforation { field: &less, shape, row_pitch: self.row_pitch };
        let mut edges = Vec::new();
        for l in region.loops() {
            Edge::extend(l, &mut edges);
        }
        let clear = edges.iter().map(|e| hole.clearance(c, e)).fold(f64::INFINITY, f64::min);
        if clear <= 0.0 || clear.is_nan() {
            return true;
        }
        let inside = |l: &Loop2| crate::sheet_surface_cuts::holds(l, c);
        inside(&region.outer) && !region.holes.iter().any(inside)
    }

    /// The bar between a hole and the hole `o` away from it (0 or less where they meet).
    fn gap_to(&self, o: Vec2) -> f64 {
        let c = Vec2::default();
        match self.shape {
            PerforationShape::Round => o.len() - self.field.diameter,
            PerforationShape::Slot { length } => {
                let (p, q) = self.slot_core(c, length);
                segment_distance(p, q, p + o, q + o) - self.field.diameter
            }
            PerforationShape::Hex => {
                let k = self.hex_corners(c);
                (0..6).map(|i| self.clearance(o, &Edge::Line(k[i], k[(i + 1) % 6]))).fold(f64::MAX, f64::min)
            }
        }
    }

    /// Whether the field's numbers make a grid: finite, a hole wider than nothing, a pitch wider
    /// than a hole, rows apart, and a slot longer than it is wide.
    fn makes_a_grid(&self) -> bool {
        let p = self.field;
        let dy = self.row_spacing();
        let finite = [p.diameter, p.pitch, p.margin, dy].iter().all(|v| v.is_finite());
        let slot = match self.shape {
            PerforationShape::Slot { length } => length.is_finite() && length > p.diameter,
            _ => true,
        };
        finite && slot && p.diameter > 0.0 && p.pitch > p.diameter && p.margin >= 0.0 && dy > 0.0
    }

    /// Whether the holes leave a bar between neighbours, on their row and on the rows beside.
    pub fn check_spacing(&self) -> Result<()> {
        let (p, dy) = (self.field.pitch, self.row_spacing());
        if !self.makes_a_grid() {
            return Err(DocError::Invalid(format!(
                "perforation numbers make no grid (diameter {}, pitch {p}, row spacing {dy}, margin {})",
                self.field.diameter, self.field.margin
            )));
        }
        let shift = if self.field.square { 0.0 } else { p / 2.0 };
        let mut near = vec![Vec2::new(p, 0.0)];
        for k in -3..=3 {
            near.push(Vec2::new(shift + k as f64 * p, dy));
            near.push(Vec2::new(k as f64 * p, 2.0 * dy));
        }
        let bar = near.iter().map(|o| self.gap_to(*o)).fold(f64::MAX, f64::min);
        if !(bar > LINEAR_TOL) {
            return Err(DocError::Invalid(format!(
                "perforation holes meet their neighbours: pitch {p} and row spacing {dy} leave no bar between them (smallest {bar:.4})"
            )));
        }
        Ok(())
    }

    /// The area the field covers: inside its outline where it has one, on the sheet's material
    /// `regions`. None where the outline can't be laid on the material: the planar boolean
    /// refuses it (too many edges, say), or the area is no number.
    pub(crate) fn field_area(&self, regions: &[Region2]) -> Option<f64> {
        let area: f64 = match &self.field.outline {
            None => regions.iter().map(Region2::area).sum(),
            Some(o) => {
                let on = solidcraft_kernel::planar_regions_boolean(regions, o, solidcraft_kernel::BoolOp::Intersect, 1e-9).ok()?;
                on.iter().map(Region2::area).sum()
            }
        };
        area.is_finite().then_some(area)
    }

    /// The edges the holes keep clear of on the flat `regions` (the material's, those of the
    /// regions in `avoid`, the outline's), the box of the flat, and the box the grid's rows and
    /// columns span: the flat's, or the part of it the outline's box holds. None where the field
    /// has no room.
    fn layout(&self, regions: &[Region2], avoid: &[Region2]) -> Result<Option<(Vec<Edge>, Aabb2, Aabb2)>> {
        let mut edges = Vec::new();
        for l in regions.iter().flat_map(Region2::loops).chain(avoid.iter().map(|z| &z.outer)) {
            Edge::extend(l, &mut edges);
        }
        let flat = edges.iter().fold(Aabb2::EMPTY, |b, e| b.union(&e.bounds()));
        if flat.is_empty() {
            return Ok(None);
        }
        if ![flat.min.x, flat.min.y, flat.max.x, flat.max.y].iter().all(|v| v.is_finite()) {
            return Err(DocError::Invalid("a perforation field's sheet has no finite flat".into()));
        }
        let mut field = flat;
        if let Some(outline) = &self.field.outline {
            let n = edges.len();
            for l in outline.iter().flat_map(Region2::loops) {
                Edge::extend(l, &mut edges);
            }
            field = field.intersection(&edges.get(n..).unwrap_or(&[]).iter().fold(Aabb2::EMPTY, |b, e| b.union(&e.bounds())));
            if !(field.min.x <= field.max.x && field.min.y <= field.max.y) {
                return Ok(None);
            }
        }
        Ok(Some((edges, flat, field)))
    }

    /// The field's rows and the places for holes along each, over `field`: refused past
    /// [`MAX_SITES`] places.
    fn sites(&self, field: &Aabb2) -> Result<()> {
        let half = self.half_extent();
        let (cx, cy) = (half.x + self.field.margin, half.y + self.field.margin);
        let rows = ((field.max.y - field.min.y - 2.0 * cy).max(0.0) / self.row_spacing()).floor() + 1.0;
        let columns = ((field.max.x - field.min.x - 2.0 * cx).max(0.0) / self.field.pitch).floor() + 2.0;
        if !(rows * columns <= MAX_SITES) {
            return Err(DocError::Invalid(format!(
                "a perforation field whose grid has more than {MAX_SITES} places for holes (its rows times the places along a row, over the field's box; it has {}) exceeds the bounded processing budget: make its pitch or row pitch larger",
                rows * columns
            )));
        }
        Ok(())
    }

    /// Whether the field's grid stays within the work limit on the flat `regions`.
    pub(crate) fn check_sites(&self, regions: &[Region2]) -> Result<()> {
        match self.layout(regions, &[])? {
            Some((_, _, field)) => self.sites(&field),
            None => Ok(()),
        }
    }

    /// Hole centres on a flat `regions`: a staggered (odd rows shifted half a pitch) or square
    /// grid, its rows `row_spacing` apart, from the lower-left corner of the flat (or of the
    /// outline's box, within the flat's) a hole's half width and the margin in, and its half
    /// height and the margin up. A hole is kept where its outline is at least the margin clear of
    /// the material's edges and cut-outs, of the regions in `avoid` and of the outline (less
    /// [`GRAZE`]), measured exactly: to lines and arcs in closed form, and to other curves on the
    /// curves themselves (see [`Edge`]). Which region holds a hole's centre is read on chords within
    /// 0.001 mm: a kept hole holds its centre at least half its width from every edge, more than
    /// that for any hole over 0.002 mm across, so the chords can't put it on the wrong side.
    pub(crate) fn grid(&self, regions: &[Region2], avoid: &[Region2]) -> Result<Vec<Vec2>> {
        let p = self.field;
        self.check_spacing()?;
        let Some((edges, flat, field)) = self.layout(regions, avoid)? else { return Ok(Vec::new()) };
        self.sites(&field)?;
        let (pitch, dy, margin) = (p.pitch, self.row_spacing(), p.margin);
        // Edges bucketed in a grid over the flat, so each hole only checks the edges near it.
        let clear = self.reach() + margin;
        let Aabb2 { min: lo, max: hi } = flat;
        let cell = (2.0 * clear).max(pitch).max(1.0).max((hi.x - lo.x) / MAX_CELLS).max((hi.y - lo.y) / MAX_CELLS);
        let buckets = Buckets::new(&edges, flat, cell, clear, MAX_ENTRIES)?;
        // The first and last rows and columns: a hole's half width (or height) and the margin in.
        let half = self.half_extent();
        let (cx, cy) = (half.x + margin, half.y + margin);
        let Aabb2 { min: flo, max: fhi } = field;
        let mut out = Vec::new();
        let mut row = 0usize;
        loop {
            let y = flo.y + cy + row as f64 * dy;
            if y > fhi.y - cy + 1e-9 {
                break;
            }
            let shift = if !p.square && row % 2 == 1 { pitch / 2.0 } else { 0.0 };
            // Steps across the row from its first hole; a mirrored field counts from the right.
            let mut step = 0.0;
            while flo.x + cx + shift + step <= fhi.x - cx + 1e-9 {
                let x = if p.from_right { fhi.x - cx - shift - step } else { flo.x + cx + shift + step };
                let c = Vec2::new(x, y);
                let near = buckets.near(c);
                if regions.iter().any(|r| r.contains(c))
                    && p.outline.as_ref().is_none_or(|o| o.iter().any(|r| r.contains(c)))
                    && !avoid.iter().any(|z| z.contains(c))
                    && near.iter().all(|&k| edges.get(k).is_none_or(|e| self.clearance(c, e) >= margin - GRAZE))
                {
                    out.push(c);
                }
                step += pitch;
            }
            row += 1;
        }
        Ok(out)
    }
}

/// The edges near each cell of a grid over a flat, so each hole only checks the edges near it.
struct Buckets {
    lo: Vec2,
    cell: f64,
    nx: usize,
    ny: usize,
    /// Each cell's edges, row by row from the flat's lower-left corner.
    cells: Vec<Vec<usize>>,
}

impl Buckets {
    /// The `edges` in the cells `cell` wide over the box `flat` that lie `clear` or nearer to
    /// them: each edge's pieces boxed (see [`Edge::pieces`]), so a long line or arc fills cells
    /// along itself rather than every cell of its box. Refused past `max_entries` in all.
    fn new(edges: &[Edge], flat: Aabb2, cell: f64, clear: f64, max_entries: usize) -> Result<Self> {
        let Aabb2 { min: lo, max: hi } = flat;
        let (nx, ny) = (((hi.x - lo.x) / cell) as usize + 1, ((hi.y - lo.y) / cell) as usize + 1);
        let mut b = Buckets { lo, cell, nx, ny, cells: vec![Vec::new(); nx.saturating_mul(ny)] };
        let reach = Vec2::new(clear, clear);
        let mut entries = 0usize;
        for (k, e) in edges.iter().enumerate() {
            for piece in e.pieces(cell) {
                let ((i0, j0), (i1, j1)) = (b.index(piece.min - reach), b.index(piece.max + reach));
                for j in j0..=j1 {
                    for i in i0..=i1 {
                        let Some(near) = b.cells.get_mut(j * nx + i) else { continue };
                        // The edge's pieces come in turn, so a cell holds it already if it is last.
                        if near.last() != Some(&k) {
                            entries += 1;
                            if entries > max_entries {
                                return Err(DocError::Invalid(format!(
                                    "a perforation field whose sheet has edges near more than {max_entries} cells of its grid, each edge counted in every cell near it, exceeds the bounded processing budget: make its pitch larger"
                                )));
                            }
                            near.push(k);
                        }
                    }
                }
            }
        }
        Ok(b)
    }

    /// The cell that holds `p`, or the nearest one.
    fn index(&self, p: Vec2) -> (usize, usize) {
        let at = |v: f64, from: f64, n: usize| ((v - from) / self.cell).floor().clamp(0.0, (n - 1) as f64) as usize;
        (at(p.x, self.lo.x, self.nx), at(p.y, self.lo.y, self.ny))
    }

    /// The edges near `p`.
    fn near(&self, p: Vec2) -> &[usize] {
        let (i, j) = self.index(p);
        self.cells.get(j * self.nx + i).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// A piece of an edge that holes keep clear of: a line, an arc of a circle, or any other curve (a
/// conic or a Bézier), each measured on itself. A curve's nearest points are found on its chords
/// within [`CHORD`] first, then on the curve by Newton from there, so a chord decides nothing.
#[derive(Clone, Copy, Debug)]
enum Edge {
    Line(Vec2, Vec2),
    Arc { center: Vec2, radius: f64, start: f64, sweep: f64 },
    Curve(Seg2),
}

impl Edge {
    /// The edges of the loop `l`, onto `out`.
    fn extend(l: &Loop2, out: &mut Vec<Edge>) {
        for s in &l.segs {
            match *s {
                Seg2::Line { a, b } => out.push(Edge::Line(a, b)),
                Seg2::Arc { center, radius, start, sweep } => out.push(Edge::Arc { center, radius, start, sweep }),
                other => out.push(Edge::Curve(other)),
            }
        }
    }

    fn start(&self) -> Vec2 {
        match *self {
            Edge::Line(a, _) => a,
            Edge::Arc { center, radius, start, .. } => center + Vec2::from_angle(start) * radius,
            Edge::Curve(s) => s.start(),
        }
    }

    fn end(&self) -> Vec2 {
        match *self {
            Edge::Line(_, b) => b,
            Edge::Arc { center, radius, start, sweep } => center + Vec2::from_angle(start + sweep) * radius,
            Edge::Curve(s) => s.end(),
        }
    }

    /// Its box: its ends, the points of an arc's circle furthest along each axis that the arc
    /// passes, and a curve's turning points along each axis.
    fn bounds(&self) -> Aabb2 {
        let mut b: Aabb2 = [self.start(), self.end()].into_iter().collect();
        match *self {
            Edge::Arc { center, radius, start, sweep } => {
                for k in 0..4 {
                    let a = k as f64 * FRAC_PI_2;
                    if sweeps(start, sweep, a) {
                        b.add(center + Vec2::from_angle(a) * radius);
                    }
                }
            }
            Edge::Curve(s) => {
                for axis in [Vec2::new(1.0, 0.0), Vec2::new(-1.0, 0.0), Vec2::new(0.0, 1.0), Vec2::new(0.0, -1.0)] {
                    b.add(furthest(&s, axis));
                }
            }
            Edge::Line(..) => {}
        }
        b
    }

    /// Boxes that together hold the edge: a line or an arc in pieces at most `len` long (at
    /// least one, at most [`MAX_PIECES`]), each piece's own box; another curve, its whole box.
    fn pieces(&self, len: f64) -> Vec<Aabb2> {
        let count = |length: f64| {
            let n = (length / len).ceil();
            if n >= 1.0 { n.min(MAX_PIECES) as usize } else { 1 }
        };
        match *self {
            Edge::Line(a, b) => {
                let n = count(a.dist(b));
                let at = |i: usize| a + (b - a) * (i as f64 / n as f64);
                (0..n).map(|i| [at(i), at(i + 1)].into_iter().collect()).collect()
            }
            Edge::Arc { center, radius, start, sweep } => {
                let sweep = sweep.clamp(-TAU, TAU);
                let n = count((radius * sweep).abs());
                let step = sweep / n as f64;
                (0..n).map(|i| Edge::Arc { center, radius, start: start + step * i as f64, sweep: step }.bounds()).collect()
            }
            Edge::Curve(_) => vec![self.bounds()],
        }
    }

    /// The distance from the point `p`.
    fn distance(&self, p: Vec2) -> f64 {
        match *self {
            Edge::Line(a, b) => to_segment(p, a, b),
            Edge::Arc { center, radius, start, sweep } => {
                let v = p - center;
                if sweeps(start, sweep, v.angle()) { (v.len() - radius).abs() } else { p.dist(self.start()).min(p.dist(self.end())) }
            }
            Edge::Curve(s) => s.point_at(s.closest_param(p)).dist(p),
        }
    }

    /// The distance from the segment `ab` (0 where they meet).
    fn distance_to_segment(&self, a: Vec2, b: Vec2) -> f64 {
        let (center, radius, start, sweep) = match *self {
            Edge::Line(p, q) => return segment_distance(a, b, p, q),
            Edge::Curve(s) => return curve_to_segment(&s, a, b),
            Edge::Arc { center, radius, start, sweep } => (center, radius, start, sweep),
        };
        let d = b - a;
        let (dd, f) = (d.dot(d), a - center);
        if dd > 0.0 {
            // Where the segment's line meets the circle: |f + t d| = radius.
            let (half, c) = (f.dot(d), f.dot(f) - radius * radius);
            let disc = half * half - dd * c;
            if disc >= 0.0 {
                for t in [(-half - disc.sqrt()) / dd, (-half + disc.sqrt()) / dd] {
                    if (0.0..=1.0).contains(&t) && sweeps(start, sweep, (f + d * t).angle()) {
                        return 0.0;
                    }
                }
            }
        }
        // Nearest at an end of either, or where the segment comes nearest the circle's centre.
        let mut m = self.distance(a).min(self.distance(b)).min(to_segment(self.start(), a, b)).min(to_segment(self.end(), a, b));
        if dd > 0.0 {
            let t = (center - a).dot(d) / dd;
            let v = f + d * t;
            if t > 0.0 && t < 1.0 && sweeps(start, sweep, v.angle()) {
                m = m.min((v.len() - radius).abs());
            }
        }
        m
    }
}

/// The point of the curve `s` furthest along the unit vector `axis`: the furthest of its chords'
/// points, or the turning point Newton finds on the curve from there (where `s′ · axis` is 0), as
/// far as that lies further.
fn furthest(s: &Seg2, axis: Vec2) -> Vec2 {
    let pts = s.polyline(CHORD);
    let Some(far) = pts.iter().copied().max_by(|p, q| p.dot(axis).total_cmp(&q.dot(axis))) else { return s.start() };
    let mut t = s.closest_param(far);
    for _ in 0..50 {
        let Some(d) = s.derivatives(t, 2) else { break };
        let (Some(d1), Some(d2)) = (d.first(), d.get(1)) else { break };
        let (f, df) = (d1.dot(axis), d2.dot(axis));
        if df.abs() <= 1e-300 || !df.is_finite() {
            break;
        }
        let next = (t - f / df).clamp(0.0, 1.0);
        let done = (next - t).abs() < 1e-15;
        t = next;
        if done {
            break;
        }
    }
    let turn = s.point_at(t);
    if turn.is_finite() && turn.dot(axis) > far.dot(axis) { turn } else { far }
}

/// The distance between the curve `s` and the segment `ab` (0 where they meet). The chords of `s`
/// within [`CHORD`] find where it comes nearest the segment, to within that: each chord as near
/// as the nearest one and twice that seeds Newton on the curve and the segment together, from the
/// chord's point nearest the segment, and the ends of both are tried too.
fn curve_to_segment(s: &Seg2, a: Vec2, b: Vec2) -> f64 {
    let pts = s.polyline(CHORD);
    let chords: Vec<(Vec2, Vec2, f64)> = pts.iter().zip(pts.iter().skip(1)).map(|(p, q)| (*p, *q, segment_distance(a, b, *p, *q))).collect();
    let nearest = chords.iter().map(|c| c.2).fold(f64::INFINITY, f64::min);
    let on = |p: Vec2| s.point_at(s.closest_param(p)).dist(p);
    let mut best = on(a).min(on(b)).min(to_segment(s.start(), a, b)).min(to_segment(s.end(), a, b));
    for &(p, q, d) in &chords {
        if d <= nearest + 2.0 * CHORD {
            best = best.min(settle(s, a, b, s.closest_param(nearest_on(p, q, a, b))));
        }
    }
    best
}

/// The point of the chord `pq` nearest the segment `ab`: where they cross, else the nearer of
/// each one's ends taken onto the other.
fn nearest_on(p: Vec2, q: Vec2, a: Vec2, b: Vec2) -> Vec2 {
    let (d, e) = (q - p, b - a);
    let den = d.cross(e);
    if den != 0.0 {
        let (u, v) = ((a - p).cross(e) / den, (a - p).cross(d) / den);
        if (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v) {
            return p + d * u;
        }
    }
    let onto = |x: Vec2, from: Vec2, to: Vec2| {
        let w = to - from;
        let l = w.dot(w);
        from + w * if l > 0.0 { ((x - from).dot(w) / l).clamp(0.0, 1.0) } else { 0.0 }
    };
    // Each pair: the chord's point, and how far it is from the segment.
    [(p, to_segment(p, a, b)), (q, to_segment(q, a, b)), (onto(a, p, q), to_segment(a, p, q)), (onto(b, p, q), to_segment(b, p, q))]
        .into_iter()
        .min_by(|x, y| x.1.total_cmp(&y.1))
        .map_or(p, |x| x.0)
}

/// Newton on how far the curve `s` at its parameter `t` lies from the segment `ab`, both moving,
/// from `t`: the distance it settles at. Where the distance is not convex in both, a step takes
/// the curve's nearest point to the segment's, then the segment's to the curve's.
fn settle(s: &Seg2, a: Vec2, b: Vec2, mut t: f64) -> f64 {
    let e = b - a;
    let ee = e.dot(e);
    let foot = |x: Vec2| if ee > 0.0 { ((x - a).dot(e) / ee).clamp(0.0, 1.0) } else { 0.0 };
    let mut u = foot(s.point_at(t));
    for _ in 0..50 {
        let Some(d) = s.derivatives(t, 2) else { break };
        let (Some(&c1), Some(&c2)) = (d.first(), d.get(1)) else { break };
        // The gradient and Hessian of half the squared distance in (u, t).
        let gap = a + e * u - s.point_at(t);
        let (gu, gt) = (gap.dot(e), -gap.dot(c1));
        let (huu, hut, htt) = (ee, -e.dot(c1), c1.dot(c1) - gap.dot(c2));
        let det = huu * htt - hut * hut;
        let (nu, nt) = if huu > 0.0 && htt > 0.0 && det > 1e-12 * huu * htt {
            ((u - (htt * gu - hut * gt) / det).clamp(0.0, 1.0), (t - (huu * gt - hut * gu) / det).clamp(0.0, 1.0))
        } else {
            let nt = s.closest_param(a + e * u);
            (foot(s.point_at(nt)), nt)
        };
        let done = (nu - u).abs() < 1e-15 && (nt - t).abs() < 1e-15;
        (u, t) = (nu, nt);
        if done {
            break;
        }
    }
    let c = s.point_at(t);
    c.dist(a + e * foot(c))
}

/// Whether an arc from the angle `start` sweeping `sweep` passes the direction at `angle`.
fn sweeps(start: f64, sweep: f64, angle: f64) -> bool {
    if sweep.abs() >= TAU {
        return true;
    }
    let (from, span) = if sweep >= 0.0 { (start, sweep) } else { (start + sweep, -sweep) };
    (angle - from).rem_euclid(TAU) <= span
}

/// The distance from `p` to the segment `ab`.
fn to_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let v = b - a;
    let len2 = v.dot(v);
    let t = if len2 > 0.0 { ((p - a).dot(v) / len2).clamp(0.0, 1.0) } else { 0.0 };
    p.dist(a + v * t)
}

/// The distance between segments `ab` and `cd` (0 where they cross).
fn segment_distance(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> f64 {
    let side = |p: Vec2, q: Vec2, r: Vec2| (q - p).cross(r - p);
    let (d1, d2, d3, d4) = (side(c, d, a), side(c, d, b), side(a, b, c), side(a, b, d));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        return 0.0;
    }
    to_segment(a, c, d).min(to_segment(b, c, d)).min(to_segment(c, a, b)).min(to_segment(d, a, b))
}

#[cfg(test)]
mod tests {
    use solidcore_record::Plane;
    use solidcraft_geom::bspline::GAUSS_8;

    use super::*;

    fn field(diameter: f64, pitch: f64, margin: f64) -> SheetPerforation {
        SheetPerforation { feature: 1, diameter, pitch, margin, square: false, from_right: false, outline: None }
    }

    fn shaped(p: &SheetPerforation, shape: PerforationShape) -> ShapedPerforation<'_> {
        ShapedPerforation { field: p, shape, row_pitch: None }
    }

    /// A flat plate `w` × `h`, 1 thick, made from surfaces, with a round cut-out (centre, radius)
    /// when asked.
    fn plate(w: f64, h: f64, cut_out: Option<(Vec2, f64)>) -> crate::SurfaceSheet {
        plate_with(w, h, cut_out.map(|(c, r)| Loop2::circle(c, r).reversed()).into_iter().collect())
    }

    /// A flat plate `w` × `h`, 1 thick, made from surfaces, with the cut-outs `holes` (clockwise).
    fn plate_with(w: f64, h: f64, holes: Vec<Loop2>) -> crate::SurfaceSheet {
        let rule = crate::ConvertRule {
            thickness: 1.0,
            k_factor: 0.44,
            bend_radius: 1.0,
            gap: 1.0,
            corner_relief: crate::sheet::ReliefShape::Notch,
            corner_relief_size: 10.0,
            relief_width: 1.0,
            relief_depth: 0.5,
        };
        let outer = Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(w, 0.0), Vec2::new(w, h), Vec2::new(0.0, h)]);
        match crate::SurfaceSheet::base("Plate", "Steel", &rule, Plane::XY, Region2 { outer, holes }, &[], 1) {
            Ok((sheet, _)) => sheet,
            Err(e) => panic!("{e}"),
        }
    }

    /// A loop's second moments of area about `c`, by Green's theorem along its segments: ∫x² dA
    /// is ∮ x³/3 dy and ∫y² dA is −∮ y³/3 dx, by Gauss–Legendre on 16 pieces of each segment.
    fn second_moments(l: &Loop2, c: Vec2) -> (f64, f64) {
        let (mut xx, mut yy) = (0.0, 0.0);
        for s in &l.segs {
            for k in 0..16 {
                for (x, w) in GAUSS_8 {
                    let t = (k as f64 + (x + 1.0) / 2.0) / 16.0;
                    let (p, d) = (s.point_at(t) - c, s.derivative(t));
                    xx += w / 32.0 * p.x.powi(3) / 3.0 * d.y;
                    yy -= w / 32.0 * p.y.powi(3) / 3.0 * d.x;
                }
            }
        }
        (xx, yy)
    }

    #[test]
    fn each_hole_shape_has_the_area_and_perimeter_of_its_loop() {
        let p = field(3.0, 8.0, 1.0);
        let c = Vec2::new(4.0, -2.0);
        for shape in [PerforationShape::Round, PerforationShape::Hex, PerforationShape::Slot { length: 7.0 }] {
            let f = shaped(&p, shape);
            let l = f.hole_loop(c);
            assert!(l.is_ccw() && l.is_closed(1e-12), "{shape:?}");
            assert!((l.signed_area() - f.hole_area()).abs() < 1e-9, "{shape:?}: {} vs {}", l.signed_area(), f.hole_area());
            let perimeter: f64 = l.segs.iter().map(Seg2::length).sum();
            assert!((perimeter - f.hole_perimeter()).abs() < 1e-9, "{shape:?}: {perimeter} vs {}", f.hole_perimeter());
            let outline = l.polyline(1e-6);
            let reach = outline.iter().map(|q| q.dist(c)).fold(0.0, f64::max);
            assert!((reach - f.reach()).abs() < 1e-6, "{shape:?}: reach {reach}");
            // How far it reaches along any direction, and its second moments about its centre
            // along the rows and across them, are its outline's.
            for k in 0..12 {
                let n = Vec2::from_angle(k as f64 * PI / 6.0 + 0.1);
                let far = outline.iter().map(|q| (*q - c).dot(n)).fold(f64::MIN, f64::max);
                assert!((far - f.support(n)).abs() < 1e-5, "{shape:?}: {far} vs {}", f.support(n));
            }
            let ((xx, yy), (along, across)) = (second_moments(&l, c), f.hole_second_moments());
            assert!((xx - along).abs() < 1e-9 * along && (yy - across).abs() < 1e-9 * across, "{shape:?}: {xx}, {yy} vs {along}, {across}");
        }
        // Ø5 on a 6 mm staggered pitch: the 63 % perforated sheet is sold as.
        let p = field(5.0, 6.0, 1.0);
        assert!((shaped(&p, PerforationShape::Round).nominal_open_share() - 0.6298).abs() < 1e-4);
    }

    /// Every hole's outline stays the margin clear of the edges, and the first row and column lie on it.
    #[test]
    fn holes_keep_the_margin_by_their_shape() {
        for shape in [PerforationShape::Hex, PerforationShape::Slot { length: 5.0 }] {
            let mut s = plate(50.0, 30.0, None);
            s.perforations.push(field(2.0, 6.0, 1.5));
            s.perforation_shapes.push((1, shape, Some(3.0)));
            let s = s.shown();
            let f = s.shaped_perforation(&s.perforations[0]);
            f.check_spacing().unwrap();
            let holes = s.perforation_holes(f.field);
            assert!(holes.len() > 40, "{shape:?}: {}", holes.len());
            let b: Aabb2 = holes.iter().flat_map(|c| f.hole_loop(*c).polyline(1e-6).iter().copied().collect::<Vec<_>>()).collect();
            assert!((b.min.x - 1.5).abs() < 1e-6 && (b.min.y - 1.5).abs() < 1e-6, "{shape:?}: {b:?}");
            assert!(b.max.x <= 48.5 + 1e-6 && b.max.y <= 28.5 + 1e-6, "{shape:?}: {b:?}");
        }
    }

    /// A round cut-out keeps every hole the margin clear of it by the hole's own outline, to the
    /// cut-out's circle itself rather than to chords of it.
    #[test]
    fn holes_keep_the_margin_from_a_round_cut_out_by_their_outline() {
        let (centre, radius, margin) = (Vec2::new(25.0, 15.0), 6.0, 1.5);
        for shape in [PerforationShape::Hex, PerforationShape::Slot { length: 5.0 }] {
            let mut s = plate(50.0, 30.0, Some((centre, radius)));
            s.perforations.push(field(2.0, 6.0, margin));
            s.perforation_shapes.push((1, shape, Some(3.0)));
            let s = s.shown();
            let f = s.shaped_perforation(&s.perforations[0]);
            let holes = s.perforation_holes(f.field);
            // From the cut-out's centre to a hole's outline: a hexagon's sides, a slot's straight
            // part less its half width.
            let apart = |c: Vec2| match shape {
                PerforationShape::Slot { length } => {
                    to_segment(centre, c - Vec2::new(length / 2.0 - 1.0, 0.0), c + Vec2::new(length / 2.0 - 1.0, 0.0)) - 1.0
                }
                _ => f.hole_loop(c).segs.iter().map(|g| to_segment(centre, g.start(), g.end())).fold(f64::MAX, f64::min),
            };
            let nearest = holes.iter().map(|c| apart(*c) - radius).fold(f64::MAX, f64::min);
            assert!(nearest >= margin - 1e-9, "{shape:?}: {nearest}");
            assert!(nearest < margin + 3.0, "{shape:?}: the cut-out keeps holes away: {nearest}");
            // And the holes the plate without it has there are gone.
            let mut whole = plate(50.0, 30.0, None);
            whole.perforations = s.perforations.clone();
            whole.perforation_shapes = s.perforation_shapes.clone();
            assert!(whole.shown().perforation_holes(f.field).len() > holes.len(), "{shape:?}");
        }
    }

    /// The circle round `c` of radius `r` as four conic quarters (rational quadratics of weight
    /// cos 45°) from 45°, counter-clockwise: a curve the grid measures as a curve, not as arcs.
    fn conic_circle(c: Vec2, r: f64) -> Loop2 {
        let at = |deg: f64, len: f64| c + Vec2::from_angle(deg.to_radians()) * len;
        let quarter = |k: u8| {
            let a = 45.0 + 90.0 * f64::from(k);
            Seg2::Conic { a: at(a, r), apex: at(a + 45.0, r * 2f64.sqrt()), b: at(a + 90.0, r), w: std::f64::consts::FRAC_1_SQRT_2 }
        };
        Loop2 { segs: (0..4).map(quarter).collect() }
    }

    /// A cut-out whose edge is a curve other than a line or an arc (a circle drawn as conics, as an
    /// ellipse is) keeps holes the margin clear of the curve itself. A slot (2 wide, 5 long, rows 3
    /// apart, margin 1.5) has a hole at (28, 14.5), its straight part from x 26.5 to 29.5; the
    /// cut-out's centre is 8 below it, so the hole keeps 8 − R − 1 from the circle. At R = 5.4998 that
    /// is 1.5002 and the hole is kept; at R = 5.5002 it is 1.4998, inside the margin, and the hole is
    /// gone. Measured on the circle's chords within 0.001 mm it was kept both times: the chord
    /// below it lies 0.0009 inside the circle.
    #[test]
    fn holes_keep_the_margin_from_a_free_form_cut_out_by_the_curve_itself() {
        let (centre, hole) = (Vec2::new(28.0, 6.5), Vec2::new(28.0, 14.5));
        for (radius, kept) in [(5.5 - 2e-4, true), (5.5 + 2e-4, false)] {
            let mut s = plate_with(50.0, 30.0, vec![conic_circle(centre, radius).reversed()]);
            s.perforations.push(field(2.0, 6.0, 1.5));
            s.perforation_shapes.push((1, PerforationShape::Slot { length: 5.0 }, Some(3.0)));
            let s = s.shown();
            let holes = s.perforation_holes(&s.perforations[0]);
            assert!(holes.iter().any(|c| c.dist(Vec2::new(22.0, 14.5)) < 1e-9), "R {radius}: the row is laid out: {holes:?}");
            assert_eq!(holes.iter().any(|c| c.dist(hole) < 1e-9), kept, "R {radius}: {holes:?}");
        }
    }

    /// With no margin, a hexagon that a cut-out's edge crosses is not kept, whether the edge is a
    /// line, an arc or another curve, with both its ends outside the hexagon. A hexagon that only
    /// touches an edge is kept. Hexagons 2 across their flats, at a 6 pitch with rows 3 apart: the
    /// first touches the plate's left and bottom edges, at (1, 2/√3). The one at (13, y2), two rows
    /// up, spans x 12…14 and reaches 2/√3 up: its top corner comes 0.3 past a rectangle's bottom
    /// edge. A circle of radius 5 comes 0.3 into its right side: two arcs from the circle's top, or
    /// four conics.
    #[test]
    fn with_no_margin_a_hexagon_an_edge_crosses_is_not_kept() {
        let y2 = 2.0 / 3f64.sqrt() + 6.0;
        let (target, beside, first) = (Vec2::new(13.0, y2), Vec2::new(7.0, y2), Vec2::new(1.0, 2.0 / 3f64.sqrt()));
        let rectangle =
            Loop2::polygon(&[Vec2::new(10.0, y2 + 0.85), Vec2::new(16.0, y2 + 0.85), Vec2::new(16.0, y2 + 2.0), Vec2::new(10.0, y2 + 2.0)]);
        let centre = Vec2::new(18.7, y2);
        let arc = |start: f64| Seg2::Arc { center: centre, radius: 5.0, start, sweep: PI };
        let arcs = Loop2 { segs: vec![arc(FRAC_PI_2), arc(-FRAC_PI_2)] };
        for (name, cut_out) in [("line", rectangle), ("arc", arcs), ("curve", conic_circle(centre, 5.0))] {
            let mut s = plate_with(50.0, 30.0, vec![cut_out.reversed()]);
            s.perforations.push(field(2.0, 6.0, 0.0));
            s.perforation_shapes.push((1, PerforationShape::Hex, Some(3.0)));
            let s = s.shown();
            let holes = s.perforation_holes(&s.perforations[0]);
            let kept = |c: Vec2| holes.iter().any(|h| h.dist(c) < 1e-9);
            assert!(kept(first) && kept(beside), "{name}: {holes:?}");
            assert!(!kept(target), "{name}: the hexagon the edge crosses is kept");
        }
    }

    /// A long line or arc goes into the cells near it, not into every cell of its box: across a
    /// flat 1000 cells wide, a diagonal or a quarter circle fills thousands of cells rather than
    /// a million, and every point less than the clearance from it finds it there. Past the
    /// budget, the field is refused.
    #[test]
    fn a_long_edge_goes_into_the_cells_near_it() {
        let flat = Aabb2 { min: Vec2::new(0.0, 0.0), max: Vec2::new(1000.0, 1000.0) };
        // Each edge, and its point and unit normal at t from 0 to 1.
        let edges: [(Edge, fn(f64) -> (Vec2, Vec2)); 2] = [
            (Edge::Line(Vec2::new(0.0, 0.0), Vec2::new(1000.0, 1000.0)), |t| {
                (Vec2::new(1000.0 * t, 1000.0 * t), Vec2::new(-1.0, 1.0) * 0.5f64.sqrt())
            }),
            (Edge::Arc { center: Vec2::new(0.0, 0.0), radius: 1000.0, start: 0.0, sweep: FRAC_PI_2 }, |t| {
                let u = Vec2::from_angle(t * FRAC_PI_2);
                (u * 1000.0, u)
            }),
        ];
        for (edge, at) in edges {
            let b = Buckets::new(&[edge], flat, 1.0, 0.5, usize::MAX).unwrap();
            let entries: usize = b.cells.iter().map(Vec::len).sum();
            assert!(entries < 20_000, "{edge:?}: {entries} entries");
            for k in 0..=4000 {
                let (p, n) = at(f64::from(k) / 4000.0);
                for off in [-0.49, 0.0, 0.49] {
                    assert!(b.near(p + n * off).contains(&0), "{edge:?}: {:?}", p + n * off);
                }
            }
            assert!(Buckets::new(&[edge], flat, 1.0, 0.5, 1000).is_err(), "{edge:?}: past the budget");
        }
    }

    /// A round field given its pattern's own row pitch is laid out as the plain grid lays it out.
    #[test]
    fn a_round_field_with_its_own_row_pitch_is_the_plain_grid() {
        for square in [false, true] {
            let mut s = plate(50.0, 30.0, None);
            let p = SheetPerforation { square, ..field(1.0, 1.5, 1.0) };
            s.perforations.push(p.clone());
            let plain = s.perforation_holes(&p);
            s.perforation_shapes.push((1, PerforationShape::Round, Some(if square { 1.5 } else { 1.5 * 3f64.sqrt() / 2.0 })));
            let rows = s.perforation_holes(&p);
            assert!(plain.len() > 500, "{}", plain.len());
            assert_eq!(plain.len(), rows.len());
            assert!(plain.iter().zip(&rows).all(|(a, b)| a.dist(*b) < 1e-9));
        }
    }

    /// A field whose outline the planar boolean refuses (here a loop left open by 0.0001 mm) keeps
    /// its holes and their open area; only the field's area and open share are unknown.
    #[test]
    fn a_field_whose_outline_cannot_be_laid_on_the_material_keeps_its_figures() {
        // In the chart the plate's flat runs y 0..-30.
        let mut s = plate(50.0, 30.0, None);
        let (a, b, c, d) = (Vec2::new(10.0, -25.0), Vec2::new(40.0, -25.0), Vec2::new(40.0, -5.0), Vec2::new(10.0, -5.0));
        let open = Loop2 {
            segs: vec![Seg2::Line { a, b }, Seg2::Line { a: b, b: c }, Seg2::Line { a: c, b: d }, Seg2::Line { a: d, b: a + Vec2::new(0.0, 1e-4) }],
        };
        s.perforations.push(SheetPerforation { outline: Some(vec![Region2 { outer: open, holes: Vec::new() }]), ..field(1.0, 3.0, 1.0) });
        for shape in [None, Some((PerforationShape::Hex, None))] {
            if let Some((shape, row_pitch)) = shape {
                s.perforation_shapes.push((1, shape, row_pitch));
            }
            let p = &s.perforations[0];
            let stats = s.perforation_stats(p).unwrap();
            assert!(stats.holes > 20, "{stats:?}");
            assert_eq!(stats.open_area, s.shaped_perforation(p).open_area(stats.holes));
            assert_eq!((stats.field_area, stats.open_share), (None, None), "{stats:?}");
        }
    }

    #[test]
    fn holes_that_meet_their_neighbours_are_refused() {
        let check = |square: bool, d: f64, pitch: f64, shape: PerforationShape, row_pitch: Option<f64>| {
            let p = SheetPerforation { square, ..field(d, pitch, 1.0) };
            ShapedPerforation { field: &p, shape, row_pitch }.check_spacing().is_ok()
        };
        let ok = |d: f64, pitch: f64, shape: PerforationShape, row_pitch: Option<f64>| check(false, d, pitch, shape, row_pitch);
        // A honeycomb: hexagons 5 across flats on a 6 pitch leave 1 between flats; on a 5 pitch none.
        assert!(ok(5.0, 6.0, PerforationShape::Hex, None));
        assert!(!ok(5.0, 5.0, PerforationShape::Hex, None));
        // Slots 8 long on an 8 pitch touch end to end; on 9 they don't, and their rows (7.8 apart) don't either.
        assert!(!ok(2.0, 8.0, PerforationShape::Slot { length: 8.0 }, None));
        assert!(ok(2.0, 9.0, PerforationShape::Slot { length: 8.0 }, None));
        // Rows 2 apart: slots 2 wide on staggered rows meet those beside, which reach over them.
        assert!(!ok(2.0, 9.0, PerforationShape::Slot { length: 8.0 }, Some(2.0)));
        assert!(ok(2.0, 9.0, PerforationShape::Slot { length: 8.0 }, Some(2.5)));
        // Hexagons 3 across flats on square rows 3.2 apart: their corners, 3.46 apart, reach into
        // the next row; on staggered rows they pass between its corners.
        assert!(!check(true, 3.0, 3.2, PerforationShape::Hex, Some(3.2)));
        assert!(check(true, 3.0, 3.2, PerforationShape::Hex, Some(3.5)));
        assert!(ok(3.0, 3.2, PerforationShape::Hex, Some(3.2)));
    }
}
