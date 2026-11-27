//! A cell grid on a cylinder wall as data: the wall as the kernel measured it, how deep the cells
//! go, and the plan the kernel returns (`solidcore::cell_grid::plan_wall`): each cell is an
//! annular sector, a [`SectorMaster`] revolved once and placed many times. Plain data: the
//! unrolling, the closure round the wall and the placement are the kernel's.

/// A whole-turn cylinder wall of a body, outside or bore. Lengths along the axis are measured
/// from `axis_point`; the unrolled wall has x as arc length from `x_dir` along `tangent` and y
/// along `axis`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CylinderWall {
    /// A point on the axis.
    pub axis_point: [f64; 3],
    /// The axis, unit, with the sign of the record's frame.
    pub axis: [f64; 3],
    /// The record frame's x axis: angle 0, unrolled x = 0.
    pub x_dir: [f64; 3],
    /// The direction x grows in at `x_dir`: `axis × x_dir` on an outside wall, `x_dir × axis` in
    /// a bore.
    pub tangent: [f64; 3],
    /// The wall's radius.
    pub radius: f64,
    /// True for an outside wall, false for a bore.
    pub convex: bool,
    /// The wall's lower end along the axis.
    pub z_lo: f64,
    /// The wall's upper end along the axis.
    pub z_hi: f64,
    /// The radius of the other surface of the wall where one faces it over all of
    /// `[z_lo, z_hi]`.
    pub facing_radius: Option<f64>,
    /// The lower end is a planar cap square to the axis.
    pub open_lo: bool,
    /// The upper end is a planar cap square to the axis.
    pub open_hi: bool,
    /// The body's length tolerance.
    pub tol: f64,
}

/// How deep the cells go into the wall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WallDepth {
    /// Through to the facing surface.
    Through,
    /// This far in from the picked wall.
    Blind(f64),
}

/// One cutter shape: the section `[r_in, r_out] × [z0, z1]` in a plane through the axis,
/// revolved about the plane's y axis by `sweep`.
#[derive(Clone, Debug, PartialEq)]
pub struct SectorMaster {
    /// The section plane's origin, on the axis.
    pub plane_origin: [f64; 3],
    /// The section plane's x axis (radial).
    pub plane_x: [f64; 3],
    /// The section plane's y axis (the wall's axis).
    pub plane_y: [f64; 3],
    /// The section's low corner `(r_in, z0)`.
    pub section_lo: [f64; 2],
    /// The section's high corner `(r_out, z1)`.
    pub section_hi: [f64; 2],
    /// The angle revolved, in radians.
    pub sweep: f64,
}

/// One cell: a master moved into place.
#[derive(Clone, Debug, PartialEq)]
pub struct WallSector {
    /// Index into [`WallCellPlan::masters`].
    pub master: u32,
    /// Row-major placement of the master.
    pub placement: [[f64; 4]; 4],
    /// The cell's lattice index along the wall.
    pub i: i64,
    /// The cell's lattice index along the axis.
    pub j: i64,
}

/// The cells of a cell grid on a cylinder wall.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct WallCellPlan {
    /// The distinct cutter shapes.
    pub masters: Vec<SectorMaster>,
    /// The cells.
    pub sectors: Vec<WallSector>,
    /// How many cells go round the wall.
    pub round: u32,
    /// The cells' area on the unrolled wall.
    pub open_area: f64,
    /// What the user should know about the plan.
    pub warnings: Vec<String>,
}
