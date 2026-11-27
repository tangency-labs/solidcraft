//! A cell grid on a cylinder wall (#23010): the cells are planned by solidcore on the unrolled
//! wall (`solidcraft_kernel::plan_wall_cell_grid`), each cell is an annular sector, a master
//! revolved once and moved into place, and the sectors, apart from each other by the web, are one
//! tool: the wall's body is cut once, not once per cell. Only square cells cut into the wall are
//! built yet: raised cells, a `direction`, a field sketch and sketched cells are refused.

use std::collections::BTreeMap;

use solidcore_record::cell_grid::CellGridSpec;
use solidcore_record::cell_grid_wall::{SectorMaster, WallDepth};
use solidcore_record::{Loop2, Plane, Region2, Vec2, Vec3};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::{self as kernel, Body};

use crate::eval::cell_grid::{MAX_CUT_CELLS, SLOW_CUT_CELLS};
use crate::eval::{point, val};
use crate::expr::{Kind, Value};
use crate::lineage::Traced;
use crate::{DocError, Document, Feature, FeatureKind, ModelState, Result};

/// `kernel::revolve` turns a profile toward its plane's normal (`x × y`): about the plane's y
/// axis, that is left-handed. A [`SectorMaster`]'s `sweep` is right-handed about its `plane_y`,
/// so it is given to `revolve` with this sign.
const REVOLVE_SIGN: f64 = -1.0;

/// The kernel's refusal of a cell grid, in its own words.
fn refused(e: kernel::KernelError) -> DocError {
    match e {
        kernel::KernelError::Invalid(why) => DocError::Invalid(why),
        e => DocError::Invalid(e.to_string()),
    }
}

/// Add `note` to a feature's warning.
fn append_warning(warning: &mut Option<String>, note: String) {
    *warning = Some(match warning.take() {
        Some(w) => format!("{w}; {note}"),
        None => note,
    });
}

/// The tool of a cell grid on a cylinder wall: every cell's annular sector, as one body.
pub(crate) fn wall_tools(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    st: &ModelState,
    warning: &mut Option<String>,
) -> Result<Vec<Traced>> {
    let _ = doc;
    let FeatureKind::CellGrid {
        sketch,
        face,
        body,
        shape,
        sides,
        dot_grid,
        size,
        cell_sketch,
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
    let Some(face) = face else {
        return Err(DocError::Invalid("a cell grid on a wall needs the wall `face`".into()));
    };
    // What a wall grid does not build yet, each refused before anything is measured.
    if sketch.is_some() {
        return Err(DocError::Invalid("not supported yet: a cell grid takes a sketch or a wall `face`, not both".into()));
    }
    if *mode == crate::CellGridMode::Raised {
        return Err(DocError::Invalid("not supported yet: raised cells on a cylinder wall; cut them into it".into()));
    }
    if *direction != crate::Direction::Positive {
        return Err(DocError::Invalid("a cell grid on a cylinder wall goes into the wall: leave `direction` out".into()));
    }
    if cell_sketch.is_some() {
        return Err(DocError::Invalid("not supported yet: sketched cells on a cylinder wall; use square cells".into()));
    }
    // The wall's body, and the face on it found again by its name (as Dome and Emboss do).
    let (index, face_index) = wall_face(st, f, face.0, body)?;
    let mb = st.bodies.get(index).ok_or_else(|| DocError::Invalid("the wall's body".into()))?;
    // The cut reaches the wall's body alone.
    if !(targets.is_empty() || (targets.len() == 1 && targets.first().is_some_and(|t| *t == mb.name))) {
        return Err(DocError::Invalid(format!("a cell grid on a cylinder wall cuts the wall's body `{}` alone: leave `targets` out", mb.name)));
    }
    if st.surface_sheets.iter().any(|sheet| sheet.body == mb.name) {
        return Err(DocError::Invalid("not supported yet: a cell grid cannot cut sheet metal; use sheet.perforate".into()));
    }
    let wall = kernel::cylinder_wall(&mb.body, face_index).map_err(refused)?;
    let defaults = kernel::default_cell_grid();
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
    let mut spec = CellGridSpec {
        shape: *shape,
        sides,
        dot_grid: *dot_grid,
        size: match size {
            Some(e) => val(vals, e, Kind::Length)?,
            None => return Err(DocError::Invalid("a cell grid needs a cell `size`".into())),
        },
        web: val(vals, web, Kind::Length)?,
        angle: match angle {
            Some(e) => val(vals, e, Kind::Angle)?,
            None => 0.0,
        },
        anchor: Vec2::ZERO,
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
    // The anchor is a point of the body's space, unrolled onto the wall; without one, the
    // kernel's anchor keeps the wall's seam in the middle of a web.
    spec.anchor = match anchor {
        Some(p) => kernel::unroll_wall_point(&wall, point(vals, p)?).map_err(refused)?,
        None => kernel::default_wall_anchor(&wall, &spec).map_err(refused)?,
    };
    let depth = match depth {
        Some(e) if !*through_all => Some(val(vals, e, Kind::Length)?),
        _ => None,
    };
    let plan = kernel::plan_wall_cell_grid(&wall, &spec, wall_depth(depth)?).map_err(refused)?;
    if plan.sectors.is_empty() {
        return Err(DocError::Invalid("no cell fits on the wall: use smaller cells, a thinner web or `clip: free`".into()));
    }
    if plan.sectors.len() > MAX_CUT_CELLS {
        return Err(DocError::Invalid(format!(
            "the grid has {} cells, more than the {MAX_CUT_CELLS} a cell grid cuts: use larger cells or a smaller field",
            plan.sectors.len()
        )));
    }
    if plan.sectors.len() > SLOW_CUT_CELLS {
        append_warning(warning, format!("{} cells: cutting more than {SLOW_CUT_CELLS} takes a while", plan.sectors.len()));
    }
    for note in &plan.warnings {
        append_warning(warning, note.clone());
    }
    // Each distinct sector is revolved once; every cell is one of them moved into place.
    let masters: Vec<Traced> = plan.masters.iter().map(|m| master_body(m).map(Traced::unnamed)).collect::<Result<_>>()?;
    let mut tools: Vec<Traced> = Vec::with_capacity(plan.sectors.len());
    for sector in &plan.sectors {
        let master = usize::try_from(sector.master)
            .ok()
            .and_then(|k| masters.get(k))
            .ok_or_else(|| DocError::Invalid(format!("the wall plan's cell ({}, {}) names no sector shape", sector.i, sector.j)))?;
        let placed = kernel::transform_matrix(&master.body, column_major(&sector.placement))?;
        tools.push(Traced::step(placed, &[master]));
    }
    // The sectors lie apart by the web, so the kernel's own check of their boxes is not asked.
    if tools.len() > 1 {
        let bodies: Vec<Body> = tools.iter().map(|t| t.body.clone()).collect();
        if let Some(one) = kernel::join_apart(&bodies)? {
            return Ok(vec![Traced::step(one, &tools.iter().collect::<Vec<_>>())]);
        }
    }
    Ok(tools)
}

/// The body (its index in the model) and the face of it (its index in the body) that a wall
/// grid's `at` picks: the face its stored name finds, on the named body when there is one; a
/// pick without a name takes the face under it. A name that finds nothing asks for the face to
/// be picked again.
fn wall_face(st: &ModelState, f: &Feature, at: Vec3, body: &Option<crate::BodyName>) -> Result<(usize, usize)> {
    let by_name = match body {
        Some(n) => Some(st.bodies.iter().position(|b| b.name == n.as_ref()).ok_or_else(|| DocError::Unknown(format!("body `{n}`")))?),
        None => None,
    };
    if let Some(name) = f.face_names.first().filter(|n| !n.is_empty()) {
        return match by_name {
            Some(i) => {
                let mut scoped = ModelState::default();
                scoped.bodies.push(st.bodies.get(i).cloned().ok_or_else(|| DocError::reselect(name.as_str()))?);
                let hit = crate::name_lookup::locate_face(&scoped, name, at).ok_or_else(|| DocError::reselect(name.as_str()))?;
                Ok((i, hit.face))
            }
            None => {
                let hit = crate::name_lookup::locate_face(st, name, at).ok_or_else(|| DocError::reselect(name.as_str()))?;
                Ok((hit.body, hit.face))
            }
        };
    }
    let lost = || DocError::Invalid("the cell grid's wall pick is not on a face of the body; pick the wall again".into());
    match by_name {
        Some(i) => {
            let mb = st.bodies.get(i).ok_or_else(lost)?;
            Ok((i, crate::appearance::face_index_at(mb, at).ok_or_else(lost)?))
        }
        // Without a body, the nearest face of any body within its pick tolerance (not the first
        // body whose loose tolerance reaches the point).
        None => st
            .bodies
            .iter()
            .enumerate()
            .filter_map(|(i, mb)| {
                let (d, face) = crate::appearance::nearest_face(mb, at)?;
                (d <= crate::appearance::pick_tolerance(mb)).then_some((d, i, face))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, i, face)| (i, face))
            .ok_or_else(lost),
    }
}

/// How deep the cells go: `depth` into the wall, else through it (with `through_all`, or with
/// neither given).
fn wall_depth(depth: Option<f64>) -> Result<WallDepth> {
    match depth {
        None => Ok(WallDepth::Through),
        Some(d) if d.is_finite() && d > 0.0 => Ok(WallDepth::Blind(d)),
        Some(d) => Err(DocError::Invalid(format!("the depth of a cell grid must be more than 0, not {d}"))),
    }
}

/// A row-major placement (the wall plan's) as the kernel's column-major matrix (`m[col][row]`).
fn column_major(rows: &[[f64; 4]; 4]) -> [[f64; 4]; 4] {
    let mut m = [[0.0; 4]; 4];
    for (r, row) in rows.iter().enumerate() {
        for (c, x) in row.iter().enumerate() {
            m[c][r] = *x;
        }
    }
    m
}

/// One sector cutter: the master's section `[r_in, r_out] × [z0, z1]` in its plane, revolved
/// about the plane's y axis by `sweep` (right-handed about `plane_y`).
fn master_body(m: &SectorMaster) -> Result<Body> {
    let bad = || DocError::Invalid("the wall plan's sector plane is degenerate".into());
    let plane = Plane::new(Vec3::from(m.plane_origin), Vec3::from(m.plane_x), Vec3::from(m.plane_y)).ok_or_else(bad)?;
    let ([r_in, z0], [r_out, z1]) = (m.section_lo, m.section_hi);
    if !(r_in > 0.0 && r_out > r_in && z1 > z0 && m.sweep.is_finite() && m.sweep > 0.0 && m.sweep < std::f64::consts::TAU) {
        return Err(DocError::Invalid(format!(
            "the wall plan's sector [{r_in}, {r_out}] × [{z0}, {z1}] turned {} is not a sector",
            m.sweep
        )));
    }
    let rect = Region2 {
        outer: Loop2::polygon(&[Vec2::new(r_in, z0), Vec2::new(r_out, z0), Vec2::new(r_out, z1), Vec2::new(r_in, z1)]),
        holes: Vec::new(),
    };
    let mut bodies = kernel::revolve(&plane, &[rect], Vec2::ZERO, Vec2::Y, REVOLVE_SIGN * m.sweep)?;
    bodies.pop().ok_or_else(|| DocError::Invalid("the sector did not revolve".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sector `[r_in, r_out] × [z0, z1]` about world Z, its section in the XZ half-plane x > 0.
    fn sector(r: [f64; 2], z: [f64; 2], sweep: f64) -> SectorMaster {
        SectorMaster {
            plane_origin: [0.0; 3],
            plane_x: [1.0, 0.0, 0.0],
            plane_y: [0.0, 0.0, 1.0],
            section_lo: [r[0], z[0]],
            section_hi: [r[1], z[1]],
            sweep,
        }
    }

    /// `kernel::revolve` turns a profile toward its plane's normal, left-handed about the axis:
    /// the section in plane (x = X, y = Z), whose normal is −Y, goes toward −Y.
    #[test]
    fn revolve_turns_toward_the_plane_normal() {
        let plane = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };
        let rect = Region2 { outer: Loop2::polygon(&[Vec2::new(10.0, 0.0), Vec2::new(12.0, 0.0), Vec2::new(12.0, 5.0), Vec2::new(10.0, 5.0)]), holes: vec![] };
        let b = kernel::revolve(&plane, &[rect], Vec2::ZERO, Vec2::Y, 0.5).unwrap().pop().unwrap();
        let (c, s) = (11.0 * 0.25_f64.cos(), 11.0 * 0.25_f64.sin());
        assert!(b.contains(Vec3::new(c, -s, 2.5)), "toward the normal −Y");
        assert!(!b.contains(Vec3::new(c, s, 2.5)), "not toward +Y");
    }

    /// A master's sweep is right-handed about its `plane_y`: about +Z, from +X toward +Y.
    #[test]
    fn a_master_turns_right_handed_about_its_plane_y() {
        let b = master_body(&sector([10.0, 12.0], [0.0, 5.0], 0.5)).unwrap();
        let (c, s) = (11.0 * 0.25_f64.cos(), 11.0 * 0.25_f64.sin());
        assert!(b.contains(Vec3::new(c, s, 2.5)), "toward +Y");
        assert!(!b.contains(Vec3::new(c, -s, 2.5)), "not toward −Y");
        // Turned about −Z instead (the section, along −Z, now spans z −5 to 0), the same sweep
        // goes the other way.
        let down = SectorMaster { plane_y: [0.0, 0.0, -1.0], ..sector([10.0, 12.0], [0.0, 5.0], 0.5) };
        let b = master_body(&down).unwrap();
        assert!(b.contains(Vec3::new(c, -s, -2.5)) && !b.contains(Vec3::new(c, s, -2.5)));
    }

    /// One square cell of the outside wall of a tube (Ro 30, Ri 27) through the wall: its volume
    /// is s²(r_out² − r_in²)/(2r), and the cutter is valid.
    #[test]
    fn one_sector_has_the_volume_of_its_annulus_piece() {
        let (r, s, ov) = (30.0, 8.471_975_511_965_976, 0.6);
        let (r_in, r_out) = (27.0 - ov, r + ov);
        let b = master_body(&sector([r_in, r_out], [10.0, 10.0 + s], s / r)).unwrap();
        assert!(b.validity().is_empty(), "{:?}", b.validity());
        let want = s * s * (r_out * r_out - r_in * r_in) / (2.0 * r);
        let got = kernel::measure(&b).unwrap().volume;
        assert!(((got - want) / want).abs() < 1e-6, "{got} vs {want}");
    }

    #[test]
    fn a_sector_that_is_no_sector_is_refused() {
        for m in [
            sector([0.0, 2.0], [0.0, 1.0], 0.5),
            sector([3.0, 2.0], [0.0, 1.0], 0.5),
            sector([1.0, 2.0], [1.0, 1.0], 0.5),
            sector([1.0, 2.0], [0.0, 1.0], 0.0),
            sector([1.0, 2.0], [0.0, 1.0], f64::NAN),
            sector([1.0, 2.0], [0.0, 1.0], std::f64::consts::TAU),
        ] {
            assert!(matches!(master_body(&m), Err(DocError::Invalid(_))), "{m:?}");
        }
        let flat = SectorMaster { plane_y: [1.0, 0.0, 0.0], ..sector([1.0, 2.0], [0.0, 1.0], 0.5) };
        assert!(master_body(&flat).is_err());
    }

    #[test]
    fn the_depth_is_through_unless_given() {
        assert_eq!(wall_depth(None).unwrap(), WallDepth::Through);
        assert_eq!(wall_depth(Some(1.5)).unwrap(), WallDepth::Blind(1.5));
        for d in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(wall_depth(Some(d)).is_err(), "{d}");
        }
    }

    /// The plan's row-major placement becomes the kernel's column-major matrix: a turn about Z
    /// by a quarter and a shift along Z move (1, 0, 0) to (0, 1, 5).
    #[test]
    fn a_placement_goes_to_the_kernel_column_major() {
        let rows = [[0.0, -1.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 5.0], [0.0, 0.0, 0.0, 1.0]];
        let m = column_major(&rows);
        assert_eq!(m[3], [0.0, 0.0, 5.0, 1.0]);
        let p = crate::apply_point(&m, Vec3::X);
        assert!(p.dist(Vec3::new(0.0, 1.0, 5.0)) < 1e-15, "{p:?}");
        // Moved by it, a sector lands where the placement says.
        let b = master_body(&sector([10.0, 12.0], [0.0, 1.0], 0.2)).unwrap();
        let moved = kernel::transform_matrix(&b, m).unwrap();
        let (c, s) = (11.0 * 0.1_f64.cos(), 11.0 * 0.1_f64.sin());
        assert!(moved.contains(Vec3::new(-s, c, 5.5)), "a quarter turn on and 5 up");
    }
}
