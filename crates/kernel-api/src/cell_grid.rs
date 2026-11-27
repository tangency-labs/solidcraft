//! Cell grids over a profile ([`CellGrid`]): the cells of one shape that fill a sketch area (an
//! outer loop is the field, a loop inside it a keep-out), each cell's opening as regions to cut
//! or raise. The spec, the plan and the cells are the record's (`solidcore_record::cell_grid`).

use solidcore_record::cell_grid::{CellGridPlan, CellGridSpec, GridCell, SketchCell};
use solidcore_record::{Loop2, Region2};

use crate::Backend;

/// The ranges a kernel takes a cell grid's spec in ([`CellGrid::LIMITS`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellGridLimits {
    /// The smallest cell size and web, in millimetres.
    pub min_size: f64,
    /// The most sides a polygon cell has.
    pub max_sides: u32,
    /// The most lattice lines a snapped field edge may be moved in.
    pub max_snap_inset: u32,
}

/// The cell grid planning of one kernel ([`crate::OpKind::CellGrid`]).
pub trait CellGrid: Backend {
    /// The ranges this kernel takes a spec in.
    const LIMITS: CellGridLimits;
    /// The grid a new cell grid starts from.
    fn default_cell_grid(&self) -> CellGridSpec;
    /// The cells of the grid `spec` over `field`, closed loops of the sketch plane nested by
    /// depth: an outer loop is a field, a loop inside it a keep-out, a loop inside that a field
    /// again. A refused parameter or field, or a field of too many cells, is the kernel's error.
    fn plan_cell_grid(&self, field: &[Loop2], spec: &CellGridSpec) -> Result<CellGridPlan, Self::Error>;
    /// The cells of a grid of the sketched cell `cell` over `field`, nested as for
    /// [`CellGrid::plan_cell_grid`]: the cell's outline repeated with its pitch and stagger, turned
    /// and flipped as the cell says, `spec` giving the web, the clip, the margin, the anchor and
    /// the angle. A refused cell, parameter or field is the kernel's error.
    fn plan_sketch_cell_grid(&self, field: &[Loop2], cell: &SketchCell, spec: &CellGridSpec) -> Result<CellGridPlan, Self::Error>;
    /// The opening of `cell` as regions: each counter-clockwise loop an outer loop, each clockwise
    /// loop a hole of the outer loop that holds it.
    fn cell_regions(&self, cell: &GridCell) -> Result<Vec<Region2>, Self::Error>;
}
