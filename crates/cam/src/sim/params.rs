//! What a simulation is given and holds the stock on: the stock as it starts, how finely and how
//! strictly to simulate, the machine it runs on, and the grid of cells.

use super::*;

/// Most cells a stock grid may have.
pub const MAX_CELLS: usize = 20_000_000;
/// Most cells the grid has when no resolution is given.
pub const DEFAULT_CELLS: usize = 4_000_000;
/// The cell size when no resolution is given, in multiples of the tolerance (coarser when the
/// stock is too large for [`DEFAULT_CELLS`]).
pub const DEFAULT_CELL_TOLERANCES: f64 = 5.0;
/// Largest coordinate accepted (mm).
pub(super) const MAX_COORD: f64 = 1.0e6;

/// The stock before the program runs, in work coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StockShape {
    /// A box (3-axis).
    Box { min: Vec3, max: Vec3 },
}

/// How finely and how strictly to simulate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SimParams {
    /// Depths up to this count as touching the part (mm, 0.0001 to 1).
    pub tolerance: f64,
    /// The cell size (mm); `None`: [`DEFAULT_CELL_TOLERANCES`] times the tolerance, coarser
    /// when that would be more than [`DEFAULT_CELLS`] cells.
    pub resolution: Option<f64>,
}

/// The machines the simulation runs programs of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Kind {
    Three,
}

impl Kind {
    pub(super) fn of(machine: &Machine) -> Result<Kind> {
        machine.validate()?;
        match machine.rotaries.as_slice() {
            [] => Ok(Kind::Three),
            _ => Err(CamError::Unsupported("the simulation handles 3-axis machines".into())),
        }
    }
}

/// The stock to start from when none is given: the part's box.
pub fn stock_around(part: &Surface, machine: &Machine) -> Result<StockShape> {
    if part.is_empty() {
        return invalid("the part has no surface");
    }
    let b = part.bounds();
    Kind::of(machine)?;
    Ok(StockShape::Box { min: b.min, max: b.max })
}

/// The cell size for a stock whose grid spans `area` mm².
pub(super) fn cell_size(area: f64, params: &SimParams) -> Result<f64> {
    match params.resolution {
        Some(r) => {
            if !(1e-4..=1e4).contains(&r) {
                return invalid("the resolution must be from 0.0001 to 10000 mm");
            }
            Ok(r)
        }
        None => Ok((params.tolerance * DEFAULT_CELL_TOLERANCES).max((area / DEFAULT_CELLS as f64).sqrt())),
    }
}

pub(super) fn cells_for(span: f64, cell: f64) -> Result<usize> {
    let n = (span / cell).ceil().max(1.0);
    if !(n <= MAX_CELLS as f64) {
        return Err(CamError::TooLarge(format!("the stock needs more than {MAX_CELLS} cells at this resolution: give a coarser one")));
    }
    Ok(n as usize)
}

pub(super) fn too_many(n: usize) -> CamError {
    CamError::TooLarge(format!("the stock needs {n} cells at this resolution, more than {MAX_CELLS}: give a coarser one"))
}

/// The stock's grid before it is split into bands: a heightfield over XY (cell `(i, j)` centred
/// at `(x0 + (i + ½)·dx, y0 + (j + ½)·dy)`, its top `top` to start with).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Grid {
    Heights { x0: f64, y0: f64, dx: f64, dy: f64, nx: usize, ny: usize, bottom: f64, top: f64 },
}

impl Grid {
    /// Rows the grid is split along.
    pub(super) fn lines(&self) -> usize {
        match *self {
            Grid::Heights { ny, .. } => ny,
        }
    }

    /// Cells along a row (along X).
    pub fn columns(&self) -> usize {
        match *self {
            Grid::Heights { nx, .. } => nx,
        }
    }

    /// Rows.
    pub fn rows(&self) -> usize {
        self.lines()
    }

    /// Tiles along a row and rows of tiles.
    pub fn tiles(&self) -> (usize, usize) {
        (self.columns().div_ceil(TILE).max(1), self.lines().div_ceil(TILE).max(1))
    }

    /// The point at height `v` over the centre of cell `(i, j)`, in work coordinates.
    pub fn point(&self, i: usize, j: usize, v: f64) -> Vec3 {
        match *self {
            Grid::Heights { x0, y0, dx, dy, .. } => Vec3::new(x0 + (i as f64 + 0.5) * dx, y0 + (j as f64 + 0.5) * dy, v),
        }
    }

    /// The value every cell starts at: the stock's top.
    pub fn initial(&self) -> f64 {
        match *self {
            Grid::Heights { top, .. } => top,
        }
    }

    /// Rows `from..to` of the stock as it starts.
    pub(super) fn band(&self, from: usize, to: usize, visits: &Arc<AtomicU64>, tol: f64) -> Model {
        let n = to.saturating_sub(from);
        match *self {
            Grid::Heights { x0, y0, dx, dy, nx, bottom, top, .. } => Model::Heights(Heights {
                x0,
                y0,
                dx,
                dy,
                nx,
                j0: from,
                ny: n,
                bottom,
                top,
                h: vec![top; nx * n],
                visits: Arc::clone(visits),
                tiles: Tiles::new(nx, n, top),
                last: vec![UNCUT; nx * n],
                floor: None,
                under: Vec::new(),
                tol,
                slot: 0,
                flute: 0.0,
            }),
        }
    }
}

pub(super) fn grid_for(stock: &StockShape, params: &SimParams) -> Result<(Grid, f64)> {
    let finite = |v: f64| v.is_finite() && v.abs() <= MAX_COORD;
    match *stock {
        StockShape::Box { min, max } => {
            if ![min.x, min.y, min.z, max.x, max.y, max.z].into_iter().all(finite) {
                return invalid("the stock's box must be finite numbers within 1000 m");
            }
            if !(max.x > min.x && max.y > min.y && max.z >= min.z) {
                return invalid("the stock's box must have a width and a depth");
            }
            let cell = cell_size((max.x - min.x) * (max.y - min.y), params)?;
            let (nx, ny) = (cells_for(max.x - min.x, cell)?, cells_for(max.y - min.y, cell)?);
            nx.checked_mul(ny).filter(|n| *n <= MAX_CELLS).ok_or_else(|| too_many(nx.saturating_mul(ny)))?;
            let grid = Grid::Heights {
                x0: min.x,
                y0: min.y,
                dx: (max.x - min.x) / nx as f64,
                dy: (max.y - min.y) / ny as f64,
                nx,
                ny,
                bottom: min.z,
                top: max.z,
            };
            Ok((grid, cell))
        }
    }
}
