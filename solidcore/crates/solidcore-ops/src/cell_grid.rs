//! Planning a cell grid over a profile: the cells of one shape that fill a sketch area (an outer
//! loop is the field, a loop inside it a keep-out) with their closed loops, counts and open area.
//!
//! A recipe of one kernel call (`solidcore::cell_grid`, which holds the lattice, the clipping and
//! the checks of its inputs): the record's spec and loops are converted by
//! [`solidcore_convert::cell_grid`], the kernel plans, and the plan comes back as the record's.
//! It computes no geometric quantity.

use solidcore::cell_grid as kernel;
use solidcore_convert as convert;
use solidcore_record::cell_grid::{CellGridPlan, CellGridSpec, GridCell, SketchCell};
use solidcore_record::{Loop2, Region2, Vec2};

pub use solidcore::cell_grid::{
    CellGridError, FLATTEN_TOL, MAX_CELLS, MAX_CELL_REACH, MAX_CELL_SEGMENTS, MAX_COORD, MAX_SIDES,
    MAX_SNAP_INSET, MIN_SIZE,
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

/// The cells of the sketched cell `cell` on its lattice over `field` (see [`plan_cell_grid`] for
/// the field). `spec` gives `web`, `angle`, `anchor`, `clip` (`whole` or `free`; `snap` is
/// refused), `keepout_clip` and `margin`.
///
/// # Errors
///
/// [`CellGridError`]: a parameter, the profile or the field is refused, a cell overlaps a neighbour
/// or comes nearer than the web, or the field reaches more than [`MAX_CELLS`] lattice cells.
pub fn plan_sketch_cell_grid(
    field: &[Loop2],
    cell: &SketchCell,
    spec: &CellGridSpec,
) -> Result<CellGridPlan, CellGridError> {
    let plan = kernel::plan_sketch(
        &convert::cell_grid::field(field),
        &convert::cell_grid::sketch_cell(cell),
        &convert::cell_grid::spec(spec),
    )?;
    Ok(convert::cell_grid::record_plan(&plan))
}

/// The origin, in the sketch plane, of lattice cell `(i, j)` of the sketched cell `cell`.
///
/// # Errors
///
/// [`CellGridError`] when a parameter is refused.
pub fn sketch_cell_center(
    spec: &CellGridSpec,
    cell: &SketchCell,
    i: i64,
    j: i64,
) -> Result<Vec2, CellGridError> {
    kernel::sketch_cell_center(
        &convert::cell_grid::spec(spec),
        &convert::cell_grid::sketch_cell(cell),
        i,
        j,
    )
    .map(convert::record_vec2)
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
    fn a_sketched_cell_is_planned_in_record_types_and_snap_is_refused() {
        let cell = SketchCell {
            profile: square(10.0),
            origin: None,
            pitch_x: 12.0,
            pitch_y: 12.0,
            stagger: 0.0,
            cell_angle: 0.0,
            flip: false,
        };
        let spec = CellGridSpec {
            clip: solidcore_record::cell_grid::Clip::Whole,
            web: 2.0,
            anchor: Vec2::new(5.0, 5.0),
            ..default_spec()
        };
        // Cell i covers [12 i, 12 i + 10]: i = 0..=2 fit in [0, 40] (the last ends at 34).
        let plan = plan_sketch_cell_grid(&[square(40.0)], &cell, &spec).unwrap();
        assert_eq!((plan.whole, plan.partial), (9, 0));
        let snap = CellGridSpec {
            clip: solidcore_record::cell_grid::Clip::Snap,
            ..spec
        };
        assert!(matches!(
            plan_sketch_cell_grid(&[square(40.0)], &cell, &snap),
            Err(CellGridError::Parameter(_))
        ));
        let c = sketch_cell_center(&spec, &cell, 2, 1).unwrap();
        assert_eq!((c.x, c.y), (29.0, 17.0));
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
