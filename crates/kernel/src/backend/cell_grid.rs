//! solidcore's cell grids ([`CellGrid`]): each the solidcore-ops call this crate made before the
//! interface existed, its refusal's words kept whole.

use solidcore_ops::cell_grid as ops;
use solidcore_record::cell_grid::{CellGridPlan, CellGridSpec, GridCell, SketchCell};
use solidcore_record::{Loop2, Region2};
use solidcraft_kernel_api::{CellGrid, CellGridLimits};

use super::Solidcore;
use crate::{KernelError, Result};

/// solidcore's refusal, its words as the kernel states them.
fn refused(e: ops::CellGridError) -> KernelError {
    KernelError::Invalid(e.to_string())
}

impl CellGrid for Solidcore {
    const LIMITS: CellGridLimits = CellGridLimits { min_size: ops::MIN_SIZE, max_sides: ops::MAX_SIDES, max_snap_inset: ops::MAX_SNAP_INSET };

    fn default_cell_grid(&self) -> CellGridSpec {
        ops::default_spec()
    }

    fn plan_cell_grid(&self, field: &[Loop2], spec: &CellGridSpec) -> Result<CellGridPlan> {
        ops::plan_cell_grid(field, spec).map_err(refused)
    }

    fn plan_sketch_cell_grid(&self, field: &[Loop2], cell: &SketchCell, spec: &CellGridSpec) -> Result<CellGridPlan> {
        ops::plan_sketch_cell_grid(field, cell, spec).map_err(refused)
    }

    fn cell_regions(&self, cell: &GridCell) -> Result<Vec<Region2>> {
        ops::cell_regions(cell).map_err(refused)
    }
}
