//! Planning a cell grid over a profile: the cells of one shape that fill a sketch area (an outer
//! loop is the field, a loop inside it a keep-out) with their closed loops, counts and open area.
//!
//! A recipe of one kernel call (`solidcore::cell_grid`, which holds the lattice, the clipping and
//! the checks of its inputs): the record's spec and loops are converted by
//! [`solidcore_convert::cell_grid`], the kernel plans, and the plan comes back as the record's.
//! It computes no geometric quantity.

use solidcore::cell_grid as kernel;
use solidcore_convert as convert;
use solidcore_record::cell_grid::{CellGridPlan, CellGridSpec, GridCell};
use solidcore_record::{Loop2, Region2, Vec2};

pub use solidcore::cell_grid::{
    CellGridError, FLATTEN_TOL, MAX_CELLS, MAX_COORD, MAX_SIDES, MIN_SIZE,
};

/// The grid a new cell grid starts from (the kernel's defaults).
#[must_use]
pub fn default_spec() -> CellGridSpec {
    convert::cell_grid::record_spec(&kernel::CellGridSpec::default())
}

/// The cells of the grid `spec` over `field`, closed loops of the sketch plane nested by depth:
/// an outer loop is a field, a loop inside it a keep-out, a loop inside that a field again.
///
/// # Errors
///
/// [`CellGridError`]: a parameter or the field is refused (not finite, out of range, a loop not
/// closed), or the field reaches more than [`MAX_CELLS`] lattice cells.
pub fn plan_cell_grid(field: &[Loop2], spec: &CellGridSpec) -> Result<CellGridPlan, CellGridError> {
    let plan = kernel::plan(
        &convert::cell_grid::field(field),
        &convert::cell_grid::spec(spec),
    )?;
    Ok(convert::cell_grid::record_plan(&plan))
}

/// The opening of `cell` as regions: each counter-clockwise loop an outer loop, each clockwise
/// loop a hole of the outer loop that holds it.
///
/// # Errors
///
/// [`CellGridError`] when a loop has no decided orientation or a hole lies in no outer loop.
pub fn cell_regions(cell: &GridCell) -> Result<Vec<Region2>, CellGridError> {
    let k = convert::cell_grid::cell(cell);
    Ok(k.regions()?.iter().map(convert::record_region2).collect())
}

/// The centre, in the sketch plane, of lattice cell `(i, j)` of the grid `spec`.
///
/// # Errors
///
/// [`CellGridError`] when a parameter is refused.
pub fn cell_center(spec: &CellGridSpec, i: i64, j: i64) -> Result<Vec2, CellGridError> {
    kernel::cell_center(&convert::cell_grid::spec(spec), i, j).map(convert::record_vec2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(side: f64) -> Loop2 {
        let p = |x, y| Vec2::new(x, y);
        Loop2::polygon(&[p(0.0, 0.0), p(side, 0.0), p(side, side), p(0.0, side)])
    }

    #[test]
    fn a_square_field_is_planned_in_record_types() {
        let spec = CellGridSpec {
            size: 5.0,
            web: 1.0,
            ..default_spec()
        };
        let plan = plan_cell_grid(&[square(40.0)], &spec).unwrap();
        assert!(!plan.cells.is_empty());
        assert_eq!(plan.cells.len(), plan.whole + plan.partial);
        assert!(plan
            .cells
            .iter()
            .all(|c| !c.loops.is_empty() && c.area > 0.0));
        let area: f64 = plan.cells.iter().map(|c| c.area).sum();
        assert!((area - plan.open_area).abs() < 1e-6);
    }

    #[test]
    fn a_refused_parameter_is_an_error_not_a_panic() {
        let spec = CellGridSpec {
            size: f64::NAN,
            ..default_spec()
        };
        assert!(matches!(
            plan_cell_grid(&[square(10.0)], &spec),
            Err(CellGridError::Parameter(_))
        ));
        assert!(cell_center(&spec, 0, 0).is_err());
    }

    #[test]
    fn the_centre_of_the_anchor_cell_is_the_anchor() {
        let spec = CellGridSpec {
            anchor: Vec2::new(3.0, 4.0),
            ..default_spec()
        };
        let c = cell_center(&spec, 0, 0).unwrap();
        assert_eq!((c.x, c.y), (3.0, 4.0));
    }
}
