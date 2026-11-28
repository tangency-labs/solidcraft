//! solidcore's cell grids ([`CellGrid`]): each the solidcore-ops call this crate made before the
//! interface existed, its refusal's words kept whole.

use solidcore_ops::cell_grid as ops;
use solidcore_ops::cell_grid_wall as wall_ops;
use solidcore_record::brep::Brep;
use solidcore_record::cell_grid::{CellGridPlan, CellGridSpec, GridCell, SketchCell};
use solidcore_record::cell_grid_wall::{CylinderWall, WallCellPlan, WallDepth};
use solidcore_record::{Loop2, Region2, Vec2, Vec3};
use solidcraft_kernel_api::{CellGrid, CellGridLimits};

use super::Solidcore;
use crate::{KernelError, Result};

/// solidcore's refusal, its words as the kernel states them.
fn refused(e: ops::CellGridError) -> KernelError {
    KernelError::Invalid(e.to_string())
}

/// Why solidcore gave no wall: a face that is no such wall, or an argument it refuses (a face
/// index past the faces), is the caller's input in solidcore's words; anything else the kernel's
/// failure.
fn wall_refused(e: wall_ops::WallError) -> KernelError {
    match e {
        wall_ops::WallError::Wall(e) => refused(e),
        wall_ops::WallError::Ops(e @ solidcore_ops::Error::Argument) => KernelError::Invalid(format!("cylinder wall: {e}")),
        wall_ops::WallError::Ops(e) => KernelError::Failed(format!("cylinder wall: {e}")),
    }
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

    fn cylinder_wall(&self, body: &Brep, face: usize) -> Result<CylinderWall> {
        wall_ops::cylinder_wall(body, face, body.tolerance).map_err(wall_refused)
    }

    fn plan_wall_cell_grid(&self, wall: &CylinderWall, spec: &CellGridSpec, depth: WallDepth) -> Result<WallCellPlan> {
        wall_ops::plan_wall_cell_grid(wall, spec, depth).map_err(refused)
    }

    fn unroll_wall_point(&self, wall: &CylinderWall, p: Vec3) -> Result<Vec2> {
        let [x, y] = wall_ops::unroll_wall_point(wall, [p.x, p.y, p.z]).map_err(refused)?;
        Ok(Vec2::new(x, y))
    }

    fn default_wall_anchor(&self, wall: &CylinderWall, spec: &CellGridSpec) -> Result<Vec2> {
        let [x, y] = wall_ops::default_wall_anchor(wall, spec).map_err(refused)?;
        Ok(Vec2::new(x, y))
    }
}
