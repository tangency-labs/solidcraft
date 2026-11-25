//! The cell grid feature (#14044): solidcore plans the cells over the sketch's profiles, each cell
//! becomes a prism along the sketch normal, and the prisms, apart from each other by the web,
//! are one tool: the targets are cut once, not once per cell. Raised cells are one tool for each
//! set of target bodies they meet, so a cell joins only the bodies it touches.

use super::*;
use solidcore_record::cell_grid::CellGridSpec;

/// The most cells one grid cuts. The cut still takes time that grows faster than the cell count
/// (release, i7-8809G, square cells through a 3 mm plate: 256 cells 6 s, 506 cells 22 s, 992 cells
/// 83 s), so a large grid is slow but finishes. The tool holds one shell per cell and the kernel's
/// record takes at most 1024 shells, so a bigger grid fails whatever the time: 1482 cells ended in
/// "shell count". The cap stays under that limit, so the refusal names the count and the cap.
pub const MAX_CUT_CELLS: usize = 1000;
const _: () = assert!(MAX_CUT_CELLS < solidcore_record::brep::MAX_SHELLS, "one shell per cell must fit the record");

/// Past this many cells the cut takes noticeable time, and the feature says so.
pub const SLOW_CUT_CELLS: usize = 100;

/// The tool of a cell grid feature: every cell's prism, as one body (raised: one per set of bodies met).
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
        cell_sketch,
        pitch_x,
        pitch_y,
        stagger,
        cell_angle,
        flip,
        cell_origin_x,
        cell_origin_y,
        web,
        angle,
        anchor,
        clip,
        keepout_clip,
        snap_inset,
        margin,
        depth,
        through_all,
        direction,
        targets,
        mode,
        ..
    } = &f.kind
    else {
        return Err(DocError::Invalid("not a cell grid".into()));
    };
    // A cut of a sheet metal body is read back onto the sheet's panels, which a grid of cells is not
    // yet. Only the bodies the cut reaches count: its targets, else its participants, else its
    // component.
    let (_, scope) = boolean_scope(st, f, &crate::plain(targets));
    if st.surface_sheets.iter().any(|sheet| scope.contains(&sheet.body)) {
        return Err(DocError::Invalid(match mode {
            crate::CellGridMode::Cut => "not supported yet: a cell grid cannot cut sheet metal; use sheet.perforate".into(),
            crate::CellGridMode::Raised => "not supported yet: a cell grid cannot add cells to sheet metal".into(),
        }));
    }
    // Raised cells stand `depth` from the plane: "through all" has no far side to reach.
    if *mode == crate::CellGridMode::Raised && *through_all {
        return Err(DocError::Invalid("a raised cell grid stands `depth` high: give `depth`, not `through_all`".into()));
    }
    let ss = constructor_profile_in_frame(
        doc,
        st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch} (it must come earlier in the timeline)")))?,
        f,
    )?;
    let regions = solidcraft_sketch::merge_regions(&select_profiles(&ss, profiles)?);
    let defaults = solidcraft_kernel::services::cell_grid::default_spec();
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
        size: match (size, cell_sketch) {
            (Some(e), _) => val(vals, e, Kind::Length)?,
            // A sketched cell has its outline and its pitch, not a size.
            (None, Some(_)) => defaults.size,
            (None, None) => return Err(DocError::Invalid("a cell grid needs a cell `size` (or a `cell_sketch`)".into())),
        },
        web: val(vals, web, Kind::Length)?,
        angle: match angle {
            Some(e) => val(vals, e, Kind::Angle)?,
            None => 0.0,
        },
        anchor,
        clip: *clip,
        keepout_clip: *keepout_clip,
        snap_inset: match snap_inset {
            Some(e) => {
                let n = val(vals, e, Kind::Unitless)?;
                if !(n.is_finite() && n.fract() == 0.0 && (0.0..=f64::from(crate::CELL_GRID_MAX_SNAP_INSET)).contains(&n)) {
                    return Err(DocError::Invalid(format!(
                        "the snap inset is a whole number of lattice lines from 0 to {}, not {n}",
                        crate::CELL_GRID_MAX_SNAP_INSET
                    )));
                }
                n as u32
            }
            None => 0,
        },
        margin: match margin {
            Some(e) => val(vals, e, Kind::Length)?,
            None => 0.0,
        },
    };
    let loops: Vec<solidcore_record::Loop2> = regions.iter().flat_map(|r| r.loops().cloned()).collect();
    let plan = match cell_sketch {
        None => solidcraft_kernel::services::cell_grid::plan_cell_grid(&loops, &spec),
        Some(id) => {
            let origin = match (cell_origin_x, cell_origin_y) {
                (Some(x), Some(y)) => Some(Vec2::new(val(vals, x, Kind::Length)?, val(vals, y, Kind::Length)?)),
                (None, None) => None,
                _ => return Err(DocError::Invalid("a cell origin needs both its x and its y".into())),
            };
            let need = |e: &Option<crate::Expr>, what: &str| match e {
                Some(e) => val(vals, e, Kind::Length),
                None => Err(DocError::Invalid(format!("a sketched cell needs `{what}`"))),
            };
            let cell = solidcore_record::cell_grid::SketchCell {
                profile: cell_profile(
                    st.sketch(id.0).ok_or_else(|| DocError::Unknown(format!("cell sketch {id} (it must come earlier in the timeline)")))?,
                )?,
                origin,
                pitch_x: need(pitch_x, "pitch_x")?,
                pitch_y: need(pitch_y, "pitch_y")?,
                stagger: match stagger {
                    Some(e) => val(vals, e, Kind::Unitless)?,
                    None => 0.0,
                },
                cell_angle: match cell_angle {
                    Some(e) => val(vals, e, Kind::Angle)?,
                    None => 0.0,
                },
                flip: *flip,
            };
            solidcraft_kernel::services::cell_grid::plan_sketch_cell_grid(&loops, &cell, &spec)
        }
    }
    .map_err(|e| DocError::Invalid(e.to_string()))?;
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
        let pieces = solidcraft_kernel::services::cell_grid::cell_regions(cell).map_err(|e| DocError::Invalid(e.to_string()))?;
        for region in &pieces {
            tools.extend(centered_extrude(&ss.plane, region, lo, hi, None, None, CapRoles::ByHeight, None)?);
        }
    }
    // Raised cells join each body they meet, as a join extrude's regions do: packed into one tool
    // they would fuse every target any cell touches. So they are packed per set of bodies met.
    let groups = match mode {
        crate::CellGridMode::Cut => vec![tools],
        crate::CellGridMode::Raised => group_by_targets(tools, &shown, &scope)?,
    };
    // The cells lie apart by the web, so the kernel's own check of their boxes (which a
    // staggered grid's neighbours fail) is not asked.
    let mut packed: Vec<Traced> = Vec::new();
    for group in groups {
        if group.len() > 1 {
            let bodies: Vec<Body> = group.iter().map(|t| t.body.clone()).collect();
            if let Some(one) = kernel::join_apart(&bodies)? {
                packed.push(Traced::step(one, &group.iter().collect::<Vec<_>>()));
                continue;
            }
        }
        packed.extend(group);
    }
    Ok(packed)
}

/// The cell tools in groups that meet the same target bodies (each group in the order of the
/// cells). With fewer than two target bodies there is nothing to tell apart: one group. Exact
/// contact, as the join itself decides which bodies a tool joins.
fn group_by_targets(tools: Vec<Traced>, shown: &ModelState, scope: &[String]) -> Result<Vec<Vec<Traced>>> {
    let targets: Vec<&ModelBody> = shown.bodies.iter().filter(|b| scope.contains(&b.name) && !b.body.is_mesh()).collect();
    if targets.len() < 2 || tools.len() < 2 {
        return Ok(vec![tools]);
    }
    let bodies: Vec<Body> = tools.iter().map(|t| t.body.clone()).collect();
    let mut met: Vec<Vec<usize>> = vec![Vec::new(); tools.len()];
    for (ti, target) in targets.iter().enumerate() {
        let mut at = 0;
        for chunk in bodies.chunks(1024) {
            for (k, hit) in target.body.meets_each(chunk)?.into_iter().enumerate() {
                if hit && let Some(m) = met.get_mut(at + k) {
                    m.push(ti);
                }
            }
            at += chunk.len();
        }
    }
    let mut groups: BTreeMap<Vec<usize>, Vec<Traced>> = BTreeMap::new();
    for (tool, m) in tools.into_iter().zip(met) {
        groups.entry(m).or_default().push(tool);
    }
    Ok(groups.into_values().collect())
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

/// The cell of a sketched grid: the one closed profile of the sketch, as a loop in the sketch's own
/// coordinates. A sketch with no closed profile, several, or one with a hole is refused, naming
/// the sketch.
pub fn cell_profile(ss: &SolvedSketch) -> Result<solidcore_record::Loop2> {
    let chosen = select_profiles(ss, &ProfileSel::All)
        .map_err(|e| DocError::Invalid(format!("the cell sketch `{}` holds no closed profile: {e}", ss.name)))?;
    let regions = solidcraft_sketch::merge_regions(&chosen);
    match regions.as_slice() {
        [one] if one.holes.is_empty() => Ok(one.outer.clone()),
        [one] => Err(DocError::Invalid(format!(
            "the cell sketch `{}` holds a profile with {} holes: a cell is one closed profile without holes",
            ss.name,
            one.holes.len()
        ))),
        many => Err(DocError::Invalid(format!(
            "the cell sketch `{}` holds {} closed profiles: a cell is exactly one closed profile",
            ss.name,
            many.len()
        ))),
    }
}
