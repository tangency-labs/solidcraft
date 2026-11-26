//! The router's cell grids ([`solidcraft_kernel_api::CellGrid`]).

use solidcore_record::cell_grid::{CellGridPlan, CellGridSpec, GridCell};
use solidcore_record::{Loop2, Region2};
use solidcraft_kernel_api::{CellGrid, CellGridLimits, OpKind};

use super::{Which, which};
use crate::Result;
use crate::backend::Solidcore;

/// The ranges a cell grid's spec is taken in: the kernel's. This build holds solidcore only, so
/// they are solidcore's, known when it is compiled.
pub const CELL_GRID_LIMITS: CellGridLimits = <Solidcore as CellGrid>::LIMITS;

/// The grid a new cell grid starts from.
pub fn default_cell_grid() -> CellGridSpec {
    match which(OpKind::CellGrid) {
        Which::Solidcore => Solidcore.default_cell_grid(),
    }
}

/// The cells of the grid `spec` over `field` (nested closed loops: a field, a keep-out inside it,
/// and so on). A refusal is [`crate::KernelError::Invalid`] with the kernel's words.
pub fn plan_cell_grid(field: &[Loop2], spec: &CellGridSpec) -> Result<CellGridPlan> {
    match which(OpKind::CellGrid) {
        Which::Solidcore => Solidcore.plan_cell_grid(field, spec),
    }
}

/// The opening of `cell` as regions. A refusal is [`crate::KernelError::Invalid`] with the
/// kernel's words.
pub fn cell_regions(cell: &GridCell) -> Result<Vec<Region2>> {
    match which(OpKind::CellGrid) {
        Which::Solidcore => Solidcore.cell_regions(cell),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::Vec2;

    fn square(side: f64) -> Loop2 {
        let p = |x, y| Vec2::new(x, y);
        Loop2::polygon(&[p(0.0, 0.0), p(side, 0.0), p(side, side), p(0.0, side)])
    }

    #[test]
    fn the_routed_grid_is_solidcores() {
        use solidcore_ops::cell_grid as ops;
        assert_eq!(CELL_GRID_LIMITS, CellGridLimits { min_size: ops::MIN_SIZE, max_sides: ops::MAX_SIDES, max_snap_inset: ops::MAX_SNAP_INSET });
        let spec = default_cell_grid();
        assert_eq!(spec, ops::default_spec());
        let field = [square(40.0)];
        let plan = plan_cell_grid(&field, &spec).unwrap();
        assert_eq!(plan, ops::plan_cell_grid(&field, &spec).unwrap());
        let cell = plan.cells.first().unwrap();
        assert_eq!(cell_regions(cell).unwrap(), ops::cell_regions(cell).unwrap());
        // a refusal carries solidcore's words whole
        let bad = CellGridSpec { size: f64::NAN, ..spec };
        let words = ops::plan_cell_grid(&field, &bad).unwrap_err().to_string();
        assert_eq!(plan_cell_grid(&field, &bad).unwrap_err(), crate::KernelError::Invalid(words));
    }
}
