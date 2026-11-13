//! The cell grid feature (#14044): solidcore plans the cells over the sketch's profiles, each cell
//! becomes a prism along the sketch normal, and the prisms, apart from each other by the web,
//! are one tool: the targets are cut once, not once per cell.

use super::*;
use solidcore_record::cell_grid::CellGridSpec;

/// The most cells one grid cuts. The kernel's boolean and its check take time that grows with the
/// square of the faces they meet, so the cap stays low until cells are cut without them.
pub const MAX_CUT_CELLS: usize = 250;

/// Past this many cells the cut takes noticeable time, and the feature says so.
pub const SLOW_CUT_CELLS: usize = 100;

/// The tool of a cell grid feature: every cell's prism, as one body.
pub(crate) fn tools(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    st: &ModelState,
    warning: &mut Option<String>,
) -> Result<Vec<Traced>> {
    let FeatureKind::CellGrid {
        sketch,
        profiles,
        shape,
        sides,
        dot_grid,
        size,
        web,
        angle,
        anchor,
        clip,
        margin,
        depth,
        through_all,
        direction,
        targets,
        ..
    } = &f.kind
    else {
        return Err(DocError::Invalid("not a cell grid".into()));
    };
    // Sheet metal cuts become cut-outs of the flat pattern, which a grid of cells cannot be. Only
    // the bodies the cut reaches count: its targets, else its participants, else its component.
    let (_, scope) = boolean_scope(st, f, &crate::plain(targets));
    if st.sheets.iter().any(|sheet| scope.contains(&sheet.body)) {
        return Err(DocError::Invalid("not supported yet: a cell grid cannot cut sheet metal; use sheet.perforate".into()));
    }
    let ss = constructor_profile_in_frame(
        doc,
        st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch} (it must come earlier in the timeline)")))?,
        f,
    )?;
    let regions = solidcraft_sketch::merge_regions(&select_profiles(&ss, profiles)?);
    let defaults = solidcore_ops::cell_grid::default_spec();
    let sides = match sides {
        Some(e) => {
            let n = val(vals, e, Kind::Unitless)?;
            if !(n.is_finite() && n.fract() == 0.0 && (3.0..=f64::from(crate::CELL_GRID_MAX_SIDES)).contains(&n)) {
                return Err(DocError::Invalid(format!("the sides of a cell are a whole number from 3 to {}, not {n}", crate::CELL_GRID_MAX_SIDES)));
            }
            n as u32
        }
        None => defaults.sides,
    };
    // The anchor is a point of the component's space: its place in the sketch's plane.
    let anchor = match anchor {
        Some(p) => ss.plane.to_local(point(vals, p)?),
        None => Vec2::new(0.0, 0.0),
    };
    let spec = CellGridSpec {
        shape: *shape,
        sides,
        dot_grid: *dot_grid,
        size: val(vals, size, Kind::Length)?,
        web: val(vals, web, Kind::Length)?,
        angle: match angle {
            Some(e) => val(vals, e, Kind::Angle)?,
            None => 0.0,
        },
        anchor,
        clip: *clip,
        margin: match margin {
            Some(e) => val(vals, e, Kind::Length)?,
            None => 0.0,
        },
    };
    let loops: Vec<solidcore_record::Loop2> = regions.iter().flat_map(|r| r.loops().cloned()).collect();
    let plan = solidcore_ops::cell_grid::plan_cell_grid(&loops, &spec).map_err(|e| DocError::Invalid(e.to_string()))?;
    if plan.cells.is_empty() {
        return Err(DocError::Invalid("no cell fits inside the profiles: use smaller cells, a thinner web or `clip: free`".into()));
    }
    if plan.cells.len() > MAX_CUT_CELLS {
        return Err(DocError::Invalid(format!(
            "the grid has {} cells, more than the {MAX_CUT_CELLS} a cell grid cuts: use larger cells or a smaller field",
            plan.cells.len()
        )));
    }
    if plan.cells.len() > SLOW_CUT_CELLS {
        append_warning(warning, format!("{} cells: cutting more than {SLOW_CUT_CELLS} takes a while", plan.cells.len()));
    }
    for note in &plan.warnings {
        append_warning(warning, note.clone());
    }
    // How far the cells go, measured where the targets are shown.
    let shown = targets_as_shown(st, f, &crate::plain(targets))?;
    let reach = if *through_all {
        exact_reach(&shown, &ss.plane, &scope)?
    } else {
        let e = depth.as_ref().ok_or_else(|| DocError::Invalid("a cell grid needs a `depth` or `through_all`".into()))?;
        val(vals, e, Kind::Length)?
    };
    if !(reach.is_finite() && reach > 0.0) {
        return Err(DocError::Invalid(format!("the depth of a cell grid must be more than 0, not {reach}")));
    }
    // A through-all reach is the distance to cover on each side of the plane, so a symmetric cut
    // takes all of it each way; an entered depth is the total, shared by the two sides.
    let (lo, hi) = match direction {
        crate::Direction::Positive => (0.0, reach),
        crate::Direction::Negative => (-reach, 0.0),
        crate::Direction::Symmetric if *through_all => (-reach, reach),
        crate::Direction::Symmetric => (-reach / 2.0, reach / 2.0),
    };
    extrude_within_reach([lo, hi])?;
    let mut tools: Vec<Traced> = Vec::new();
    for cell in &plan.cells {
        let pieces = solidcore_ops::cell_grid::cell_regions(cell).map_err(|e| DocError::Invalid(e.to_string()))?;
        for region in &pieces {
            tools.extend(centered_extrude(&ss.plane, region, lo, hi, None, None, false)?);
        }
    }
    // The cells lie apart by the web, so the kernel's own check of their boxes (which a
    // staggered grid's neighbours fail) is not asked.
    if tools.len() > 1 {
        let bodies: Vec<Body> = tools.iter().map(|t| t.body.clone()).collect();
        if let Some(one) = kernel::join_apart(&bodies)? {
            return Ok(vec![Traced::step(one, &tools.iter().collect::<Vec<_>>())]);
        }
    }
    Ok(tools)
}

/// How far from the sketch plane the bodies named in `scope` reach, with a margin: the distance a
/// through-all cell must go on either side to leave nothing. From each body's exact bounds, not
/// from its display mesh.
fn exact_reach(st: &ModelState, plane: &Plane, scope: &[String]) -> Result<f64> {
    let mut far: f64 = 0.0;
    let mut any = false;
    for b in st.bodies.iter().filter(|b| scope.contains(&b.name)) {
        let bb = b.body.exact_bounds()?;
        any = true;
        for i in 0..8 {
            let p = Vec3::new(
                if i & 1 == 0 { bb.min.x } else { bb.max.x },
                if i & 2 == 0 { bb.min.y } else { bb.max.y },
                if i & 4 == 0 { bb.min.z } else { bb.max.z },
            );
            far = far.max(plane.height(p).abs());
        }
    }
    if !any {
        return Err(DocError::Invalid("through all needs a body to cut".into()));
    }
    Ok(far * 1.02 + 1.0)
}
