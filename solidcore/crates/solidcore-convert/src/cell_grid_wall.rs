//! The wall cell grid's record data and the kernel's, both ways, field for field: the wall and
//! the depth go to the kernel, the wall and the plan come back (`solidcore::cell_grid`).

use solidcore::cell_grid as k;
use solidcore_record::cell_grid_wall::{
    CylinderWall, SectorMaster, WallCellPlan, WallDepth, WallSector,
};

/// A record wall as the kernel's.
#[must_use]
pub fn wall(w: &CylinderWall) -> k::CylinderWall {
    k::CylinderWall {
        axis_point: w.axis_point,
        axis: w.axis,
        x_dir: w.x_dir,
        tangent: w.tangent,
        radius: w.radius,
        convex: w.convex,
        z_lo: w.z_lo,
        z_hi: w.z_hi,
        facing_radius: w.facing_radius,
        open_lo: w.open_lo,
        open_hi: w.open_hi,
        tol: w.tol,
    }
}

/// A kernel wall as the record's.
#[must_use]
pub fn record_wall(w: &k::CylinderWall) -> CylinderWall {
    CylinderWall {
        axis_point: w.axis_point,
        axis: w.axis,
        x_dir: w.x_dir,
        tangent: w.tangent,
        radius: w.radius,
        convex: w.convex,
        z_lo: w.z_lo,
        z_hi: w.z_hi,
        facing_radius: w.facing_radius,
        open_lo: w.open_lo,
        open_hi: w.open_hi,
        tol: w.tol,
    }
}

/// A record depth as the kernel's.
#[must_use]
pub fn depth(d: WallDepth) -> k::WallDepth {
    match d {
        WallDepth::Through => k::WallDepth::Through,
        WallDepth::Blind(d) => k::WallDepth::Blind(d),
    }
}

/// A kernel plan as the record's.
#[must_use]
pub fn record_plan(p: &k::WallCellPlan) -> WallCellPlan {
    WallCellPlan {
        masters: p
            .masters
            .iter()
            .map(|m| SectorMaster {
                plane_origin: m.plane_origin,
                plane_x: m.plane_x,
                plane_y: m.plane_y,
                section_lo: m.section_lo,
                section_hi: m.section_hi,
                sweep: m.sweep,
            })
            .collect(),
        sectors: p
            .sectors
            .iter()
            .map(|s| WallSector {
                master: s.master,
                placement: s.placement,
                i: s.i,
                j: s.j,
            })
            .collect(),
        round: p.round,
        open_area: p.open_area,
        warnings: p.warnings.clone(),
    }
}
