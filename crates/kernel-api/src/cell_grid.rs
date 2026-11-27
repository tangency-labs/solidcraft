//! Cell grids over a profile ([`CellGrid`]): the cells of one shape that fill a sketch area (an
//! outer loop is the field, a loop inside it a keep-out), each cell's opening as regions to cut
//! or raise. The spec, the plan and the cells are the record's (`solidcore_record::cell_grid`).

use solidcore_record::cell_grid::{CellGridPlan, CellGridSpec, GridCell, SketchCell};
use solidcore_record::brep::Brep;
use solidcore_record::cell_grid_wall::{CylinderWall, WallCellPlan, WallDepth};
use solidcore_record::{Loop2, Region2, Vec2, Vec3};

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
    /// The whole-turn cylinder wall of face `face` of `body`, outside or bore, as the unrolling
    /// measures it. A face that is not such a wall (a plane, a sphere, a cone, a part of a turn, a
    /// wall with openings) is the kernel's error, in its words.
    fn cylinder_wall(&self, body: &Brep, face: usize) -> Result<CylinderWall, Self::Error>;
    /// The cells of the grid `spec` round `wall`, each an annular sector to revolve once per
    /// distinct master and place, cut `depth` into the wall. A size that does not close round the
    /// wall, a shape not supported on a wall, or too many cells is the kernel's error.
    fn plan_wall_cell_grid(&self, wall: &CylinderWall, spec: &CellGridSpec, depth: WallDepth) -> Result<WallCellPlan, Self::Error>;
    /// The point `p` on `wall` as unrolled coordinates: arc length from the wall's angle zero along
    /// its tangent, and height along the axis from the wall's axis point.
    fn unroll_wall_point(&self, wall: &CylinderWall, p: Vec3) -> Result<Vec2, Self::Error>;
    /// The anchor a cell grid on `wall` gets when none is given, in unrolled coordinates.
    fn default_wall_anchor(&self, wall: &CylinderWall, spec: &CellGridSpec) -> Result<Vec2, Self::Error>;
}
