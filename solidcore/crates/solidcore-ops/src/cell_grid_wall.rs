//! A cell grid on a cylinder wall: the wall of a body as the kernel measures it, the cells that go
//! round it and where each one sits.
//!
//! Recipes of one kernel call each (`solidcore::cell_grid::wall` holds the unrolling, the closure
//! round the wall and the placement; `solidcore::query::cylinder_wall` reads the wall off a
//! B-rep): the record's wall, depth and spec are converted by
//! [`solidcore_convert::cell_grid_wall`], the kernel answers, and the answer comes back as the
//! record's. It computes no geometric quantity.

use solidcore::cell_grid as kernel;
use solidcore::query::cylinder_wall::cylinder_wall as kernel_cylinder_wall;
use solidcore_convert as convert;
use solidcore_record::brep::Brep;
use solidcore_record::cell_grid::CellGridSpec;
use solidcore_record::cell_grid_wall::{CylinderWall, WallCellPlan, WallDepth};

use crate::body::Body;
use crate::session::read_body;

pub use solidcore::cell_grid::CellGridError;

/// Why [`cylinder_wall`] gave no wall.
#[derive(Clone, Debug)]
pub enum WallError {
    /// The record did not become a kernel body, or the kernel call failed.
    Ops(crate::Error),
    /// The face is not a wall the cell grid supports (its kind, a partial turn, openings, ...).
    Wall(CellGridError),
}

impl From<crate::Error> for WallError {
    fn from(e: crate::Error) -> Self {
        WallError::Ops(e)
    }
}

/// The whole-turn cylinder wall that is face `face` (record order) of `brep`, outside or bore,
/// measured to the length tolerance `tol`.
///
/// # Errors
///
/// [`WallError::Wall`] when the face is not a whole-turn cylinder wall without openings (the
/// message says what it is and what to pick); [`WallError::Ops`] when the record is refused or
/// the face index is past the faces.
pub fn cylinder_wall(brep: &Brep, face: usize, tol: f64) -> Result<CylinderWall, WallError> {
    let (body, order) = Body::from_record(brep, true)?;
    let id = crate::order::face(&order, face)?;
    let wall = read_body("cylinder_wall", &body, |_, b| {
        Ok(Some(kernel_cylinder_wall(b, id, tol)))
    })?;
    match wall {
        Some(Ok(w)) => Ok(convert::cell_grid_wall::record_wall(&w)),
        Some(Err(e)) => Err(WallError::Wall(e)),
        None => Err(WallError::Ops(crate::Error::Exception)),
    }
}

/// The cells of the grid `spec` on `wall`, cut to `depth`: annular sectors, each a master shape
/// moved into place.
///
/// # Errors
///
/// [`CellGridError`]: a parameter is refused, the cells are not square at 0 or 90 degrees, the
/// cell size does not go round the wall evenly (the message names the sizes that do), the wall
/// has no facing wall for a through cut, or the field reaches too many cells.
pub fn plan_wall_cell_grid(
    wall: &CylinderWall,
    spec: &CellGridSpec,
    depth: WallDepth,
) -> Result<WallCellPlan, CellGridError> {
    let plan = kernel::plan_wall(
        &convert::cell_grid_wall::wall(wall),
        &convert::cell_grid::spec(spec),
        convert::cell_grid_wall::depth(depth),
    )?;
    Ok(convert::cell_grid_wall::record_plan(&plan))
}

/// The point `p` on the unrolled wall: x the arc length from the wall's angle zero, y along the
/// axis.
///
/// # Errors
///
/// [`CellGridError`] when the wall or `p` is not finite.
pub fn unroll_wall_point(wall: &CylinderWall, p: [f64; 3]) -> Result<[f64; 2], CellGridError> {
    kernel::unroll_point(&convert::cell_grid_wall::wall(wall), p)
}

/// The anchor a grid gets when none is given: half a pitch from the wall's angle zero, at mid
/// height, so no cutter crosses the seam.
///
/// # Errors
///
/// [`CellGridError`] when the wall or a parameter is refused.
pub fn default_wall_anchor(
    wall: &CylinderWall,
    spec: &CellGridSpec,
) -> Result<[f64; 2], CellGridError> {
    kernel::default_anchor(
        &convert::cell_grid_wall::wall(wall),
        &convert::cell_grid::spec(spec),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell_grid::default_spec;

    fn tube() -> CylinderWall {
        CylinderWall {
            axis_point: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            x_dir: [1.0, 0.0, 0.0],
            tangent: [0.0, 1.0, 0.0],
            radius: 20.0,
            convex: true,
            z_lo: 0.0,
            z_hi: 40.0,
            facing_radius: Some(17.0),
            open_lo: true,
            open_hi: true,
            tol: 1e-6,
        }
    }

    /// Square cells of pitch 2 pi 20 / 20 round the wall (size 5.2832, web 1), whole clip: the
    /// default spec is hexagons with snap, which a wall refuses.
    fn spec() -> CellGridSpec {
        let pitch = 2.0 * std::f64::consts::PI * 20.0 / 20.0;
        CellGridSpec {
            shape: solidcore_record::cell_grid::CellShape::Square,
            clip: solidcore_record::cell_grid::Clip::Whole,
            size: pitch - 1.0,
            web: 1.0,
            ..default_spec()
        }
    }

    #[test]
    fn a_tube_wall_is_planned_in_record_types() {
        let plan = plan_wall_cell_grid(&tube(), &spec(), WallDepth::Through).unwrap();
        assert_eq!(plan.round, 20);
        assert!(!plan.sectors.is_empty() && !plan.masters.is_empty());
        assert!(plan
            .sectors
            .iter()
            .all(|s| (s.master as usize) < plan.masters.len()));
    }

    #[test]
    fn a_refused_parameter_is_an_error_not_a_panic() {
        let spec = CellGridSpec {
            size: f64::NAN,
            ..spec()
        };
        assert!(plan_wall_cell_grid(&tube(), &spec, WallDepth::Through).is_err());
    }

    #[test]
    fn a_point_on_the_wall_unrolls_to_arc_length_and_height() {
        // A quarter turn round (0, 20, 5) from the x axis: 20 * pi / 2 along the wall.
        let [x, y] = unroll_wall_point(&tube(), [0.0, 20.0, 5.0]).unwrap();
        assert!((x - 10.0 * std::f64::consts::PI).abs() < 1e-9);
        assert!((y - 5.0).abs() < 1e-9);
    }

    #[test]
    fn the_default_anchor_is_half_a_pitch_round_at_mid_height() {
        let spec = spec();
        let pitch = spec.size + spec.web;
        let [x, y] = default_wall_anchor(&tube(), &spec).unwrap();
        assert!((x - pitch / 2.0).abs() < 1e-9);
        assert!((y - 20.0).abs() < 1e-9);
    }
}
