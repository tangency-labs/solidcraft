//! Cell grids: fills the area of a sketch with cells of one shape (hexagons, squares, triangles,
//! circles or regular polygons) and returns the cells as closed 2D loops, with the counts and the
//! open area. This is the pure planning half of the cell grid feature: it knows a field and the
//! grid's parameters, not a body, a command or a cut.
//!
//! # The field
//! A list of closed loops of lines and arcs in the sketch plane, nested by depth: an outer loop
//! is a field, a loop inside it is a keep-out (a boss, a screw hole, a label), a loop inside that
//! is a field again. Curves that are neither lines nor arcs (splines, conics) are followed to
//! within [`FLATTEN_TOL`] and their loop is never snapped.
//!
//! # The lattice
//! With `s` the cell `size` (its inscribed diameter), `w` the `web` between neighbours and
//! `p = s + w`, cells of a hexagon grid stack in columns along Y at pitch `p`, the columns
//! `p * sqrt(3) / 2` apart and offset by `p / 2` from each other. Cell `(i, j)` has its centre at
//! `anchor + rotate(angle) * (i * h, i * p / 2 + j * p)`, always worked out from the integer
//! indices. Two fields with the same shape, size, web, angle and anchor therefore get the same
//! lattice, which is how a grid split over two parts lines up. With `angle` 0 a hexagon is
//! flat-top; the other shapes follow the same pattern (see [`CellShape`]).
//!
//! # Clipping, at a field's edge
//! The field as drawn is the limit: no mode puts a cell outside it or over a keep-out.
//! - [`Clip::Whole`]: only cells entirely inside the field.
//! - [`Clip::Free`]: cells crossing an edge are trimmed to the field exactly (`cell ∩ field`).
//! - [`Clip::Snap`]: one unbroken lattice, with gaps only around openings, and a partial cell only
//!   where an edge cuts it along a line between two of its vertices. Each straight edge that runs
//!   along a lattice-line direction moves onto the nearest lattice line on the solid side (fields
//!   shrink, keep-outs grow). A loop with an arc, a spline or an off-direction edge keeps its
//!   drawn shape, and a cell it crosses gets the largest single-chord piece that fits inside the
//!   field, or stays solid. Triangles, circles and polygons have no chords, so only the
//!   tilings (hexagon, square, triangle) snap; circles and polygons act as `Whole`.
//!
//! Not upstream: SolidCraft's cell grid planner, a kernel primitive because it computes
//! geometry any client could use (docs/layers.md §1). The field is the kernel's own planar profile
//! ([`crate::prism::profile::Loop2`]); a curve that is neither a line nor an arc is flattened, as
//! the kernel has no exact clip of a free-form loop against a cell. No recursion.
//!
//! Inputs are hostile: sizes and the field are checked, and a field that would reach more than
//! [`MAX_CELLS`] lattice cells is refused rather than planned.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing range tests, kept as written

mod free;
pub(crate) mod lattice;
pub(crate) mod region;
pub(crate) mod snap;
mod vec;

#[cfg(test)]
mod tests;

use crate::math::Vec2;
use crate::prism::profile::{Loop2, Region2, Seg2, MAX_BEZIER_DEGREE};

use free::Cell as FreeCell;
use lattice::Lattice;
use region::{
    poly_area, poly_centroid, rotate, union_tiles, Convex, Loop, Prim, Region, MAX_LOOPS, MAX_PRIMS,
};
use vec::{ring_next, Vec2Ext};

pub use lattice::MAX_CELLS;

/// Most sides of a polygon cell.
pub const MAX_SIDES: u32 = 64;
/// Largest coordinate (mm) of the field or the anchor.
pub const MAX_COORD: f64 = 1e9;
/// Smallest cell size and web (mm).
pub const MIN_SIZE: f64 = 1e-3;
/// How closely a curve that is neither a line nor an arc is followed (mm).
pub const FLATTEN_TOL: f64 = 1e-4;
/// A loop whose ends are farther apart than this (mm) is not closed.
const OPEN_TOL: f64 = 1e-4;

/// What a cell looks like. Every shape has its inscribed diameter `size`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CellShape {
    /// Regular hexagons, flat-top at angle 0, tiling the plane with an even web.
    #[default]
    Hexagon,
    /// Axis-aligned squares of side `size`.
    Square,
    /// Equilateral triangles pointing up and down alternately; parallel sides of neighbours are
    /// one web apart. A triangle has no chord that is not a side, so a triangle is only ever cut whole.
    Triangle,
    /// Circles of diameter `size` on a [`DotGrid`].
    Circle,
    /// Regular polygons with `sides` sides and inscribed diameter `size`, a flat at the bottom,
    /// on a [`DotGrid`].
    Polygon,
}

/// The grid of centres for circles and polygons, which have no grid of their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DotGrid {
    /// Rows of centres offset by half a pitch, as hexagons stack.
    #[default]
    Hexagonal,
    /// Centres on a square grid.
    Square,
}

/// What happens to a cell the field's edge crosses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Clip {
    /// Only cells entirely inside the field.
    Whole,
    /// Edges move onto lattice lines; partial cells are cut along the cell's own chords.
    #[default]
    Snap,
    /// Cells are trimmed to the field as drawn.
    Free,
}

/// The grid's parameters, all in the sketch's units (mm and radians).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellGridSpec {
    /// The cell's shape.
    pub shape: CellShape,
    /// Sides of a [`CellShape::Polygon`] cell, 3 to [`MAX_SIDES`].
    pub sides: u32,
    /// For circles and polygons.
    pub dot_grid: DotGrid,
    /// The cell's inscribed diameter, at least [`MIN_SIZE`].
    pub size: f64,
    /// The gap between neighbouring cells, at least [`MIN_SIZE`].
    pub web: f64,
    /// The grid's rotation about the anchor, in the sketch plane.
    pub angle: f64,
    /// A cell centre sits here (sketch coordinates): this pins the lattice's phase.
    pub anchor: Vec2,
    /// What happens to a cell the field's edge crosses.
    pub clip: Clip,
}

impl Default for CellGridSpec {
    fn default() -> Self {
        CellGridSpec {
            shape: CellShape::Hexagon,
            sides: 8,
            dot_grid: DotGrid::Hexagonal,
            size: 5.0,
            web: 1.2,
            angle: 0.0,
            anchor: Vec2::ZERO,
            clip: Clip::Snap,
        }
    }
}

impl CellGridSpec {
    fn validate(&self) -> Result<(), CellGridError> {
        let bad = |what: &str, v: f64| {
            CellGridError::Parameter(format!("{what} must be a finite number, not {v}"))
        };
        if !self.size.is_finite() {
            return Err(bad("the cell size", self.size));
        }
        if !self.web.is_finite() {
            return Err(bad("the web", self.web));
        }
        if self.size < MIN_SIZE || self.size > MAX_COORD {
            return Err(CellGridError::Parameter(format!(
                "the cell size must be from {MIN_SIZE} to {MAX_COORD} mm, not {}",
                self.size
            )));
        }
        if self.web < MIN_SIZE || self.web > MAX_COORD {
            return Err(CellGridError::Parameter(format!(
                "the web must be from {MIN_SIZE} to {MAX_COORD} mm, not {}",
                self.web
            )));
        }
        if !self.angle.is_finite() || self.angle.abs() > 1e6 {
            return Err(CellGridError::Parameter(format!(
                "the angle must be finite and within a million radians, not {}",
                self.angle
            )));
        }
        if !(self.anchor.is_finite()
            && self.anchor.x.abs() <= MAX_COORD
            && self.anchor.y.abs() <= MAX_COORD)
        {
            return Err(CellGridError::Parameter(
                "the anchor must be a finite point within a billion mm of the origin".into(),
            ));
        }
        Ok(())
    }

    fn lattice(&self) -> Result<Lattice, CellGridError> {
        self.validate()?;
        Lattice::new(self.shape, self.size, self.web, self.dot_grid, self.sides)
    }
}

/// Why a cell grid could not be planned.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum CellGridError {
    #[error("cell grid: {0}")]
    /// A parameter is refused.
    Parameter(String),
    #[error("cell grid: the field: {0}")]
    /// The field is refused.
    Field(String),
    #[error("cell grid: the field reaches {cells} lattice cells, more than the {max} allowed: use larger cells or a smaller field")]
    /// The field reaches more lattice cells than allowed.
    TooManyCells {
        /// The cells it reaches.
        cells: u64,
        /// The most allowed.
        max: usize,
    },
}

/// How a cell's opening came about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellKind {
    /// The whole cell.
    Whole,
    /// A cell cut exactly along snapped edges: every cut runs along a line between two of its vertices.
    Exact,
    /// A cell next to an edge that could not snap, cut to the largest single-chord piece that fits.
    Fallback,
    /// A cell trimmed to the field as drawn (`free` clipping).
    Free,
}

/// One cell's opening.
#[derive(Clone, Debug, PartialEq)]
pub struct GridCell {
    /// Lattice indices: with the grid's parameters they name the cell.
    pub i: i64,
    /// The second lattice index.
    pub j: i64,
    /// How the opening came about.
    pub kind: CellKind,
    /// The lattice centre of the cell, in the sketch plane (the centre of a partial cell's whole cell).
    pub center: Vec2,
    /// The opening's closed loops in the sketch plane: outer loops counter-clockwise, holes
    /// clockwise. One loop unless a keep-out splits the cell or sits inside it.
    pub loops: Vec<Loop2>,
    /// The opening's area (mm²).
    pub area: f64,
}

/// The cells to open, and what they add up to.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct CellGridPlan {
    /// The cells.
    pub cells: Vec<GridCell>,
    /// Cells opened whole.
    pub whole: usize,
    /// Cells opened partially, fallback cells included.
    pub partial: usize,
    /// Partial cells next to an edge that could not snap.
    pub fallback: usize,
    /// The total area of the openings (mm²).
    pub open_area: f64,
    /// Things worth telling the user: a loop that collapsed when snapped and was kept as drawn,
    /// curves followed approximately.
    pub warnings: Vec<String>,
}

impl GridCell {
    /// The opening as regions: each counter-clockwise loop an outer loop, each clockwise loop a
    /// hole of the outer loop that holds it.
    ///
    /// # Errors
    ///
    /// [`CellGridError::Field`] when a loop has no decided orientation, or a hole lies in no outer
    /// loop.
    pub fn regions(&self) -> Result<Vec<Region2>, CellGridError> {
        let mut regions: Vec<Region2> = Vec::new();
        let mut holes: Vec<&Loop2> = Vec::new();
        for lp in &self.loops {
            let area = lp
                .signed_area()
                .map_err(|e| CellGridError::Field(format!("a cell's loop has no area: {e}")))?;
            match area.sign() {
                Some(true) => regions.push(Region2 {
                    outer: lp.clone(),
                    holes: Vec::new(),
                }),
                Some(false) => holes.push(lp),
                None => {
                    return Err(CellGridError::Field(
                        "a cell's loop has no decided orientation".into(),
                    ))
                }
            }
        }
        for hole in holes {
            let probe = hole
                .segs
                .first()
                .map(|s| s.point_at(0.5))
                .ok_or_else(|| CellGridError::Field("a cell's hole is empty".into()))?;
            // The innermost outer loop that holds it: an island inside a hole has holes of its
            // own, and they lie inside the outer loops around it as well.
            let mut owner: Option<(f64, &mut Region2)> = None;
            for r in &mut regions {
                if r.outer.holds(probe) != Some(true) {
                    continue;
                }
                let area = r
                    .outer
                    .signed_area()
                    .map_err(|e| CellGridError::Field(format!("a cell's loop has no area: {e}")))?
                    .value;
                if owner.as_ref().is_none_or(|(best, _)| area < *best) {
                    owner = Some((area, r));
                }
            }
            let (_, owner) = owner
                .ok_or_else(|| CellGridError::Field("a cell's hole lies in no loop".into()))?;
            owner.holes.push(hole.clone());
        }
        Ok(regions)
    }
}

/// The centre, in the sketch plane, of lattice cell `(i, j)` of a grid.
pub fn cell_center(spec: &CellGridSpec, i: i64, j: i64) -> Result<Vec2, CellGridError> {
    let lat = spec.lattice()?;
    Ok(spec.anchor + rotate(lat.center(i, j), spec.angle))
}

/// The cells of a grid over a field given as closed loops (see the module docs).
pub fn plan(field: &[Loop2], spec: &CellGridSpec) -> Result<CellGridPlan, CellGridError> {
    let lat = spec.lattice()?;
    let mut warnings = Vec::new();
    let loops = lattice_loops(field, spec, &mut warnings)?;
    let mut clip = spec.clip;
    if clip == Clip::Snap && !lat.snappable() {
        clip = Clip::Whole; // no lattice lines to snap to
    }
    let mut region = Region::from_loops(loops, lat.p);
    if clip == Clip::Snap {
        let (snapped, notes) = snap::snap_region(&region, &lat);
        region = snapped;
        warnings.extend(notes);
    }
    let mut out = CellGridPlan {
        warnings,
        ..CellGridPlan::default()
    };
    let Some(bounds) = region.bounds() else {
        return Ok(out);
    };
    for (i, j) in lat.cells_in(&bounds)? {
        let c = lat.center(i, j);
        let key = lat.key(i, j);
        let poly: Vec<Vec2> = lat.outline(key).iter().map(|&q| q + c).collect();
        let Some(convex) = Convex::new(poly) else {
            continue;
        };
        let hits = region.hits(&convex);
        let whole = |lat: &Lattice| -> Vec<Vec<Prim>> {
            if lat.round {
                vec![vec![Prim::arc(c, lat.s / 2.0, 0.0, std::f64::consts::TAU)]]
            } else {
                vec![points_to_prims(&convex.pts)]
            }
        };
        if hits.is_empty() {
            if region.contains(c) {
                push(
                    &mut out,
                    spec,
                    (i, j, c),
                    CellKind::Whole,
                    whole(&lat),
                    lat.cell_area,
                );
            }
            continue;
        }
        match clip {
            Clip::Whole => {}
            Clip::Free => {
                let cell = if lat.round {
                    FreeCell::Circle { c, r: lat.s / 2.0 }
                } else {
                    FreeCell::Polygon(&convex)
                };
                match free::clip_cell(&region, &cell) {
                    Some(pieces) => {
                        let area: f64 = pieces.iter().map(|p| p.1).sum();
                        if area > 1e-9 {
                            push(
                                &mut out,
                                spec,
                                (i, j, c),
                                CellKind::Free,
                                pieces.into_iter().map(|p| p.0).collect(),
                                area,
                            );
                        }
                    }
                    None => out.warnings.push(format!(
                        "cell ({i}, {j}) could not be trimmed to the field and was left solid"
                    )),
                }
            }
            Clip::Snap => {
                if hits
                    .iter()
                    .all(|&h| region.prims.get(h).is_some_and(Prim::is_lattice))
                {
                    let inside: Vec<Vec<Vec2>> = lat
                        .tiles(key)
                        .iter()
                        .map(|t| t.iter().map(|&q| q + c).collect::<Vec<_>>())
                        .filter(|t| region.contains(poly_centroid(t)))
                        .collect();
                    if inside.is_empty() {
                        continue;
                    }
                    let area: f64 = inside.iter().map(|t| poly_area(t)).sum();
                    if area >= lat.cell_area * (1.0 - 1e-9) {
                        push(
                            &mut out,
                            spec,
                            (i, j, c),
                            CellKind::Whole,
                            whole(&lat),
                            lat.cell_area,
                        );
                    } else {
                        let loops = union_tiles(&inside)
                            .iter()
                            .map(|l| points_to_prims(l))
                            .collect();
                        push(&mut out, spec, (i, j, c), CellKind::Exact, loops, area);
                    }
                    continue;
                }
                for (piece, cand) in lat.candidates(key) {
                    let q: Vec<Vec2> = cand.iter().map(|&p| p + c).collect();
                    if region.holds(q.clone()) {
                        if *piece == lattice::PieceKind::Whole {
                            push(
                                &mut out,
                                spec,
                                (i, j, c),
                                CellKind::Whole,
                                whole(&lat),
                                lat.cell_area,
                            );
                        } else {
                            let area = poly_area(&q);
                            push(
                                &mut out,
                                spec,
                                (i, j, c),
                                CellKind::Fallback,
                                vec![points_to_prims(&q)],
                                area,
                            );
                        }
                        break;
                    }
                }
            }
        }
    }
    Ok(out)
}

/// Adds a cell to the plan, its loops carried from the lattice frame to the sketch plane.
fn push(
    out: &mut CellGridPlan,
    spec: &CellGridSpec,
    (i, j, c): (i64, i64, Vec2),
    kind: CellKind,
    loops: Vec<Vec<Prim>>,
    area: f64,
) {
    out.cells.push(GridCell {
        i,
        j,
        kind,
        center: spec.anchor + rotate(c, spec.angle),
        loops: loops
            .into_iter()
            .filter_map(|l| loop_to_sketch(&l, spec))
            .collect(),
        area,
    });
    out.open_area += area;
    if kind == CellKind::Whole {
        out.whole += 1;
    } else {
        out.partial += 1;
    }
    if kind == CellKind::Fallback {
        out.fallback += 1;
    }
}

fn points_to_prims(pts: &[Vec2]) -> Vec<Prim> {
    let n = pts.len();
    (0..n)
        .filter_map(|k| Some(Prim::line(*pts.get(k)?, *pts.get(ring_next(k, n))?)))
        .collect()
}

/// A lattice-frame loop as a loop of the sketch plane.
fn loop_to_sketch(prims: &[Prim], spec: &CellGridSpec) -> Option<Loop2> {
    let segs: Vec<Seg2> = prims
        .iter()
        .map(|p| match p.moved(spec.angle, spec.anchor) {
            Prim::Line { a, b, .. } => Seg2::Line { a, b },
            Prim::Arc {
                c, r, a0, sweep, ..
            } => Seg2::Arc {
                center: c,
                radius: r,
                start: a0,
                sweep,
            },
        })
        .collect();
    (!segs.is_empty()).then_some(Loop2 { segs })
}

/// The field's loops in the lattice frame (the sketch frame moved to the anchor and turned by
/// the angle), welded at their joints.
fn lattice_loops(
    field: &[Loop2],
    spec: &CellGridSpec,
    warnings: &mut Vec<String>,
) -> Result<Vec<Loop>, CellGridError> {
    if field.len() > MAX_LOOPS {
        return Err(CellGridError::Field(format!(
            "{} loops, more than the {MAX_LOOPS} allowed",
            field.len()
        )));
    }
    let turn = -spec.angle;
    let shift = rotate(-spec.anchor, turn);
    let mut total = 0usize;
    let mut flattened = false;
    let mut loops = Vec::new();
    for (n, lp) in field.iter().enumerate() {
        let wrong = |what: &str| CellGridError::Field(format!("loop {n}: {what}"));
        if lp.segs.is_empty() {
            return Err(wrong("has no segments"));
        }
        let mut prims: Vec<Prim> = Vec::new();
        let mut curved = false;
        for seg in &lp.segs {
            check_seg(seg, MAX_COORD).map_err(wrong)?;
            match *seg {
                Seg2::Line { a, b } => prims.push(Prim::line(a, b)),
                Seg2::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                } => {
                    curved = true;
                    prims.push(Prim::arc(center, radius, start, sweep));
                }
                _ => {
                    curved = true;
                    flattened = true;
                    let pts = flatten(seg, FLATTEN_TOL)
                        .ok_or_else(|| wrong("has a curve too complex to follow"))?;
                    prims.extend(
                        pts.windows(2)
                            .filter_map(|w| Some(Prim::line(*w.first()?, *w.get(1)?))),
                    );
                }
            }
            if total + prims.len() > MAX_PRIMS {
                return Err(CellGridError::Field(format!(
                    "more than {MAX_PRIMS} boundary pieces"
                )));
            }
        }
        total += prims.len();
        if !lp.is_closed(OPEN_TOL) {
            return Err(wrong("is not closed"));
        }
        let moved: Vec<Prim> = prims.iter().map(|p| p.moved(turn, shift)).collect();
        loops.push(
            Loop::new(moved, Some(curved))
                .ok_or_else(|| wrong("has a coordinate that is not finite"))?,
        );
    }
    if flattened {
        warnings.push(format!("curves other than lines and arcs are followed to within {FLATTEN_TOL} mm, and their loop never snaps"));
    }
    Ok(loops)
}

/// Checks a segment read from outside before anything evaluates it: every stored coordinate
/// finite and at most `max_abs` in size (a Bézier's unused slots too), a radius in
/// 1e-12..=`max_abs`, an arc's start within ±1e6 radians and its absolute sweep 1e-12 to one turn
/// (with 1e-12 radians of allowance), a conic weight in 1e-12..=1e12, a Bézier degree of 1 to
/// [`MAX_BEZIER_DEGREE`], controls not all at one point, and finite ends.
fn check_seg(seg: &Seg2, max_abs: f64) -> Result<(), &'static str> {
    const MIN_POSITIVE: f64 = 1e-12;
    const MAX_WEIGHT: f64 = 1e12;
    const MAX_START_ANGLE: f64 = 1e6;
    let require = |ok: bool, what: &'static str| if ok { Ok(()) } else { Err(what) };
    let coordinates = |values: &mut dyn Iterator<Item = f64>| {
        require(
            {
                let mut ok = true;
                for v in values {
                    ok &= v.is_finite() && v.abs() <= max_abs;
                }
                ok
            },
            "non-finite or excessive coordinate",
        )
    };
    let controls = |points: &[Vec2]| -> Result<(), &'static str> {
        coordinates(&mut points.iter().flat_map(|p| [p.x, p.y]))?;
        let Some((first, rest)) = points.split_first() else {
            return Err("empty controls");
        };
        require(rest.iter().any(|p| p != first), "collapsed curve")
    };
    match *seg {
        Seg2::Line { a, b } => controls(&[a, b])?,
        Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            coordinates(&mut [center.x, center.y].into_iter())?;
            require(
                radius.is_finite() && (MIN_POSITIVE..=max_abs).contains(&radius),
                "arc radius",
            )?;
            require(
                start.is_finite() && start.abs() <= MAX_START_ANGLE,
                "arc start angle",
            )?;
            require(
                sweep.is_finite()
                    && (MIN_POSITIVE..=std::f64::consts::TAU + 1e-12).contains(&sweep.abs()),
                "arc sweep",
            )?;
        }
        Seg2::Cubic { p0, p1, p2, p3 } => controls(&[p0, p1, p2, p3])?,
        Seg2::Conic { a, apex, b, w } => {
            controls(&[a, apex, b])?;
            require(
                w.is_finite() && (MIN_POSITIVE..=MAX_WEIGHT).contains(&w),
                "conic weight",
            )?;
        }
        Seg2::Bezier { n, ref p } => {
            require(
                (1..=MAX_BEZIER_DEGREE).contains(&usize::from(n)),
                "Bezier degree",
            )?;
            coordinates(&mut p.iter().flat_map(|q| [q.x, q.y]))?;
            controls(p.get(..=usize::from(n)).ok_or("Bezier degree")?)?;
        }
    }
    require(
        seg.start().is_finite() && seg.end().is_finite(),
        "non-finite curve endpoint",
    )
}

/// The most pieces a curve is cut into.
const MAX_FLATTEN_PIECES: usize = 4096;

/// A curve that is neither a line nor an arc as a polyline (both end points included) with at
/// most about `tol` chord error: the pieces' count from the second differences of the controls
/// (chord error of `n` pieces is at most the largest second difference over `8 n^2`). `None` when
/// that takes more than [`MAX_FLATTEN_PIECES`] pieces: the curve is then too complex to follow
/// within `tol`.
fn flatten(seg: &Seg2, tol: f64) -> Option<Vec<Vec2>> {
    let tol = tol.max(1e-9);
    let pieces = |bow: f64| {
        let n = (bow / (8.0 * tol)).sqrt().ceil();
        if n.is_finite() && n <= MAX_FLATTEN_PIECES as f64 {
            Some((n as usize).max(2))
        } else {
            None
        }
    };
    let n = match *seg {
        Seg2::Cubic { p0, p1, p2, p3 } => pieces(
            (p0 - p1 * 2.0 + p2)
                .length()
                .max((p1 - p2 * 2.0 + p3).length())
                * 6.0,
        )?,
        Seg2::Conic { a, apex, b, w } => pieces((a - apex * 2.0 + b).length() * 2.0 * w.max(1.0))?,
        Seg2::Bezier { n, ref p } => {
            let c = p.get(..=usize::from(n)).unwrap_or(&[]);
            let d2 = c.windows(3).map(|w| match w {
                [a, b, c] => (*a - *b * 2.0 + *c).length(),
                _ => 0.0,
            });
            pieces(d2.fold(0.0, f64::max) * f64::from(n) * (f64::from(n) - 1.0))?
        }
        Seg2::Line { a, b } => return Some(vec![a, b]),
        Seg2::Arc { .. } => 8,
    };
    Some((0..=n).map(|i| seg.point_at(i as f64 / n as f64)).collect())
}
