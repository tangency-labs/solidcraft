//! A cell grid on a whole-turn cylinder wall, outside or bore: the wall is unrolled to a strip of
//! length `C = 2 pi r` (x the arc length from the wall's angle zero along its tangent, y along the
//! axis), the grid is planned on the strip by the unchanged [`plan`], one copy of each cell is kept
//! round the wall, and each kept cell becomes an annular sector: a [`SectorMaster`] (a rectangle
//! `[r_in, r_out] x [0, h]` in a plane through the axis, revolved about the axis) placed by a
//! rotation about the axis and a shift along it.
//!
//! # Closure
//! A lattice goes round the wall only when the translation `(C, 0)` of the unrolled strip is a
//! translation of the lattice: `a v1 + b v2 = (C, 0)` with `a`, `b` integers and `v1`, `v2` the
//! lattice's steps, read off [`cell_center`] ([`closing_steps`]). A grid that does not close is
//! refused with the two nearest sizes that do; nothing is re-pitched silently.
//!
//! # Which copy is kept
//! Cells `(i, j)` and `(i + a, j + b)` are the same cell of the wall. Of each such class the copy
//! whose column lies in the run of `|a|` (or `|b|`) columns centred on `x = C / 2` is kept, decided
//! on the integer indices, so no anchor duplicates or drops a cell.
//!
//! Only square cells at a multiple of 90 degrees become sectors yet; [`closing_steps`] covers
//! every lattice.

use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, TAU};

use crate::math::Vec2;
use crate::prism::profile::{Loop2, Seg2};

use super::{
    cell_center, plan, CellGridError, CellGridSpec, CellKind, CellShape, GridCell, MAX_COORD,
    MIN_SIZE,
};

/// The direction `revolve` turns in: `1.0` when a positive sweep turns the section plane's x axis
/// towards `plane_y x plane_x` (right-handed about `plane_y`), `-1.0` when it turns the other way.
/// Every sector's placement follows this one constant, so flipping it is the whole fix if the
/// kernel's revolve turns the other way.
pub const REVOLVE_SIGN: f64 = 1.0;

/// How far from a multiple of 90 degrees (radians) an angle may be and still count as one.
const QUARTER_TOL: f64 = 1e-12;
/// Most steps of a lattice round the wall (the cells round it fit a `u32`, with room).
const MAX_STEPS: f64 = 2_147_483_647.0;
/// How far the search for the lattice's closing period looks, in steps each way.
const PERIOD_SEARCH: i64 = 12;

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
/// revolved about the plane's y axis by `sweep` (in the direction [`REVOLVE_SIGN`] names).
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

/// The integer steps `(a, b)` with `a v1 + b v2 = (circumference, 0)`, where `v1`, `v2` are the
/// lattice's steps: the centre differences of cells `(1, 0)` and `(0, 1)` from `(0, 0)` (for
/// triangles `(2, 0)` and `(1, 1)`, which keep a cell's orientation), turned by the grid's angle.
/// The anchor does not enter.
///
/// # Errors
///
/// [`CellGridError::Parameter`] for a refused parameter or a circumference that is not a positive
/// length up to [`MAX_COORD`]; [`CellGridError::Layout`] when the lattice does not go round
/// evenly (to within `1e-9` of the circumference), naming the two nearest sizes that do.
pub fn closing_steps(spec: &CellGridSpec, circumference: f64) -> Result<(i64, i64), CellGridError> {
    let c = circumference;
    if !(c.is_finite() && c > 0.0 && c <= MAX_COORD) {
        return Err(CellGridError::Parameter(format!(
            "the wall's circumference must be a positive length up to {MAX_COORD} mm, not {c}"
        )));
    }
    let (v1, v2) = steps(spec)?;
    let det = v1.x * v2.y - v1.y * v2.x;
    if !(det.is_finite() && det != 0.0) {
        return Err(CellGridError::Parameter(
            "the lattice's steps are parallel".into(),
        ));
    }
    let a = c * v2.y / det;
    let b = -c * v1.y / det;
    if a.is_finite() && b.is_finite() && a.abs() <= MAX_STEPS && b.abs() <= MAX_STEPS {
        let (ar, br) = (a.round(), b.round());
        let rx = ar * v1.x + br * v2.x - c;
        let ry = ar * v1.y + br * v2.y;
        if (ar != 0.0 || br != 0.0) && rx.hypot(ry) <= 1e-9 * c {
            return Ok((ar as i64, br as i64));
        }
    }
    Err(not_closing(spec, c, v1, v2))
}

/// The lattice's two steps (see [`closing_steps`]), measured with the anchor at the origin so a
/// far anchor costs no precision.
fn steps(spec: &CellGridSpec) -> Result<(Vec2, Vec2), CellGridError> {
    let base = CellGridSpec {
        anchor: Vec2::ZERO,
        ..*spec
    };
    let ((i1, j1), (i2, j2)) = if spec.shape == CellShape::Triangle {
        ((2, 0), (1, 1))
    } else {
        ((1, 0), (0, 1))
    };
    let o = cell_center(&base, 0, 0)?;
    Ok((
        cell_center(&base, i1, j1)? - o,
        cell_center(&base, i2, j2)? - o,
    ))
}

/// The refusal of a grid that does not go round a wall of circumference `c`: the two nearest
/// sizes that do, with the same web, when the lattice closes along x at all.
fn not_closing(spec: &CellGridSpec, c: f64, v1: Vec2, v2: Vec2) -> CellGridError {
    let shape = match spec.shape {
        CellShape::Hexagon => "hexagon",
        CellShape::Square => "square",
        CellShape::Triangle => "triangle",
        CellShape::Circle => "circle",
        CellShape::Polygon => "polygon",
    };
    let pitch = spec.lattice().map_or(spec.size + spec.web, |l| l.p);
    let lead = format!(
        "{shape} cells of size {} and web {} (pitch {}) do not go round evenly: the wall is {c:.2} mm round",
        mm(spec.size),
        mm(spec.web),
        mm(pitch)
    );
    // The shortest lattice translation along x (the period round the wall) and the least x step
    // between columns, both found among small step combinations.
    let scale = v1.x.hypot(v1.y).max(v2.x.hypot(v2.y));
    let mut period = f64::INFINITY;
    let mut column = f64::INFINITY;
    for a in -PERIOD_SEARCH..=PERIOD_SEARCH {
        for b in -PERIOD_SEARCH..=PERIOD_SEARCH {
            let w = v1 * a as f64 + v2 * b as f64;
            if w.x > 1e-9 * scale {
                column = column.min(w.x);
                if w.y.abs() <= 1e-9 * scale {
                    period = period.min(w.x);
                }
            }
        }
    }
    if !(period.is_finite() && column.is_finite()) {
        return CellGridError::Layout(format!(
            "{lead}; at this angle the lattice never closes round it"
        ));
    }
    // The period scales with the pitch for every shape but the polygon, whose pitch is not
    // the size plus the web.
    if spec.shape == CellShape::Polygon {
        return CellGridError::Layout(format!("{lead}; choose a size whose period divides it"));
    }
    let per_period = (period / column).round().max(1.0);
    let n_lo = (c / period).floor();
    let mut fits = Vec::new();
    for n in [n_lo, n_lo + 1.0] {
        let size = pitch * (c / n) / period - spec.web;
        if n >= 1.0 && size.is_finite() && size >= MIN_SIZE {
            fits.push((size, n * per_period));
        }
    }
    match fits.as_slice() {
        [(s1, n1), (s2, n2)] => CellGridError::Layout(format!(
            "{lead}; size {s1:.3} closes with {n1} cells, {s2:.3} with {n2}"
        )),
        [(s1, n1)] => CellGridError::Layout(format!("{lead}; size {s1:.3} closes with {n1} cells")),
        _ => CellGridError::Layout(format!("{lead}; no size closes with this web")),
    }
}

/// A length for a message: at most three decimals, trailing zeros dropped.
fn mm(v: f64) -> String {
    let s = format!("{v:.3}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// The cells of the grid `spec` on `wall`, cut to `depth`: one annular sector per cell, each a
/// master shape placed by a rotation about the axis and a shift along it. `spec.anchor` is a
/// point of the unrolled wall ([`unroll_point`], [`default_anchor`]).
///
/// # Errors
///
/// [`CellGridError::Parameter`] for a refused wall or parameter, cells other than squares at a
/// multiple of 90 degrees, a depth the wall cannot take, or cells reaching an end of the wall that
/// meets another face; [`CellGridError::Layout`] when the cells do not go round evenly;
/// [`CellGridError::TooManyCells`] from the strip's plan.
pub fn plan_wall(
    wall: &CylinderWall,
    spec: &CellGridSpec,
    depth: WallDepth,
) -> Result<WallCellPlan, CellGridError> {
    let sigma = check_wall(wall)?;
    spec.validate()?;
    let quarter = spec.angle / FRAC_PI_2;
    if spec.shape != CellShape::Square
        || !((quarter - quarter.round()).abs() * FRAC_PI_2 <= QUARTER_TOL)
    {
        return Err(CellGridError::Parameter(
            "on a cylinder wall only square cells at 0° or 90° are supported yet".into(),
        ));
    }
    let r = wall.radius;
    let c = TAU * r;
    let (a, b) = closing_steps(spec, c)?;
    let (r_in, r_out, ov) = radii(wall, depth)?;
    let (v1, v2) = steps(spec)?;
    // The run of indices that goes round once: along i when the wall's x follows i, else along j.
    let (along_i, period, step) = match (a, b) {
        (a, 0) if a != 0 => (true, a.abs(), v1.x),
        (0, b) if b != 0 => (false, b.abs(), v2.x),
        _ => {
            return Err(CellGridError::Parameter(format!(
                "internal: square cells close round the wall by the steps ({a}, {b})"
            )))
        }
    };
    let p = spec.size + spec.web;
    let pad = 2.0 * p + spec.size + spec.margin;
    let field = [rect_loop(-pad, wall.z_lo, c + pad, wall.z_hi)];
    let grid = plan(&field, spec)?;
    let mut out = WallCellPlan {
        round: period as u32,
        warnings: grid.warnings.clone(),
        ..WallCellPlan::default()
    };
    let index = |cell: &GridCell| if along_i { cell.i } else { cell.j };
    let Some(first) = grid.cells.first() else {
        return Ok(out);
    };
    // The column nearest x = C / 2, and the run of `period` columns centred on it.
    let mid = index(first) + ((c / 2.0 - first.center.x) / step).round() as i64;
    let base = mid - (period - 1) / 2;
    let mut keys: HashMap<[u64; 4], u32> = HashMap::new();
    for cell in &grid.cells {
        if (index(cell) - base).div_euclid(period) != 0 {
            continue;
        }
        let [mut x0, mut z0, mut x1, mut z1] = cell_box(cell, wall.tol).ok_or_else(|| {
            CellGridError::Parameter(format!(
                "internal: cell ({}, {}) is not a rectangle along the wall",
                cell.i, cell.j
            ))
        })?;
        let whole = cell.kind == CellKind::Whole;
        if whole {
            // Whole cells from the lattice's own numbers, so their masters are bit-equal: the
            // width and height below are `spec.size` itself, never a difference of two floats.
            let h = spec.size / 2.0;
            (x0, x1, z0, z1) = (
                cell.center.x - h,
                cell.center.x - h + spec.size,
                cell.center.y - h,
                cell.center.y - h + spec.size,
            );
        }
        let closed_end = || {
            CellGridError::Parameter(
                "cells reach the wall's end where it meets another face; use clip: whole or a margin"
                    .into(),
            )
        };
        let (mut lo, mut hi) = (z0, z1);
        let mut pushed = false;
        if (z0 - wall.z_lo).abs() <= wall.tol {
            if !wall.open_lo {
                return Err(closed_end());
            }
            lo = wall.z_lo - ov;
            pushed = true;
        }
        if (z1 - wall.z_hi).abs() <= wall.tol {
            if !wall.open_hi {
                return Err(closed_end());
            }
            hi = wall.z_hi + ov;
            pushed = true;
        }
        let sweep = (if whole { spec.size } else { x1 - x0 }) / r;
        let height = if whole && !pushed { spec.size } else { hi - lo };
        let key = [
            sweep.to_bits(),
            height.to_bits(),
            r_in.to_bits(),
            r_out.to_bits(),
        ];
        let master = match keys.get(&key) {
            Some(&m) => m,
            None => {
                let m = out.masters.len() as u32;
                out.masters.push(SectorMaster {
                    plane_origin: wall.axis_point,
                    plane_x: wall.x_dir,
                    plane_y: wall.axis,
                    section_lo: [r_in, 0.0],
                    section_hi: [r_out, height],
                    sweep,
                });
                keys.insert(key, m);
                m
            }
        };
        // The master spans angles REVOLVE_SIGN * [0, sweep] (right-handed about the axis); the
        // cell spans sigma * [x0 / r, x1 / r].
        let start = x0 / r;
        let turn = if REVOLVE_SIGN * sigma > 0.0 {
            sigma * start
        } else {
            sigma * (start + sweep)
        };
        out.sectors.push(WallSector {
            master,
            placement: placement(wall.axis_point, wall.axis, turn, lo),
            i: cell.i,
            j: cell.j,
        });
        out.open_area += cell.area;
    }
    Ok(out)
}

/// The radii `(r_in, r_out)` of every sector, and the overlap `ov` past each surface the cut
/// leaves.
fn radii(wall: &CylinderWall, depth: WallDepth) -> Result<(f64, f64, f64), CellGridError> {
    let (r, tol) = (wall.radius, wall.tol);
    if let Some(f) = wall.facing_radius {
        if (wall.convex && f >= r - tol) || (!wall.convex && f <= r + tol) {
            return Err(CellGridError::Parameter(format!(
                "internal: the facing surface at radius {} is on the wrong side of the wall at {}",
                mm(f),
                mm(r)
            )));
        }
    }
    let thickness = wall.facing_radius.map(|f| (f - r).abs());
    let ov = (0.2 * thickness.unwrap_or(r)).clamp(0.1, 2.0);
    let depth = match depth {
        WallDepth::Through => WallDepth::Through,
        WallDepth::Blind(d) => {
            if !(d.is_finite() && d > 0.0) {
                return Err(CellGridError::Parameter(format!(
                    "the depth must be a positive length, not {d}"
                )));
            }
            match thickness {
                Some(t) if d > t + tol => {
                    return Err(CellGridError::Parameter(format!(
                        "depth {} is more than the wall's {} mm; leave depth out to cut through",
                        mm(d),
                        mm(t)
                    )))
                }
                Some(t) if (d - t).abs() <= tol => WallDepth::Through,
                _ => WallDepth::Blind(d),
            }
        }
    };
    let (r_in, r_out) = match (wall.convex, depth, wall.facing_radius) {
        (true, WallDepth::Through, None) => {
            return Err(CellGridError::Parameter(
                "the body has no bore under this wall; give a `depth`".into(),
            ))
        }
        (false, WallDepth::Through, None) => {
            return Err(CellGridError::Parameter(
                "the body has no outside wall over this bore; give a `depth`".into(),
            ))
        }
        (true, WallDepth::Through, Some(f)) => (f - ov, r + ov),
        (true, WallDepth::Blind(d), _) => (r - d, r + ov),
        (false, WallDepth::Through, Some(f)) => (r - ov, f + ov),
        (false, WallDepth::Blind(d), _) => (r - ov, r + d),
    };
    if !(r_in > 10.0 * tol && r_out > r_in) {
        return Err(CellGridError::Parameter(format!(
            "the cells would reach the wall's axis (inner radius {}); give a smaller depth",
            mm(r_in)
        )));
    }
    Ok((r_in, r_out, ov))
}

/// The point `p` on the unrolled wall: x the arc length from `x_dir` along `tangent`, in
/// `[0, 2 pi r)`, y the distance along the axis from `axis_point`.
///
/// # Errors
///
/// [`CellGridError::Parameter`] when the wall is refused, `p` is not finite, or `p` lies on the
/// axis.
pub fn unroll_point(wall: &CylinderWall, p: [f64; 3]) -> Result<[f64; 2], CellGridError> {
    check_wall(wall)?;
    if !p.iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD) {
        return Err(CellGridError::Parameter(
            "the point must be finite and within a billion mm of the origin".into(),
        ));
    }
    let d = sub(p, wall.axis_point);
    let z = dot(d, wall.axis);
    let (u, v) = (dot(d, wall.x_dir), dot(d, wall.tangent));
    if !(u.hypot(v) > wall.tol) {
        return Err(CellGridError::Parameter(
            "the point is on the wall's axis".into(),
        ));
    }
    let mut angle = v.atan2(u);
    if angle < 0.0 {
        angle += TAU;
    }
    if angle >= TAU {
        angle = 0.0;
    }
    Ok([wall.radius * angle, z])
}

/// The anchor a grid gets when none is given: half a pitch round from the wall's angle zero, at
/// mid-height. The wall's seam then lies mid-web, half a web from every cell.
///
/// # Errors
///
/// [`CellGridError::Parameter`] when the wall or a parameter is refused.
pub fn default_anchor(wall: &CylinderWall, spec: &CellGridSpec) -> Result<[f64; 2], CellGridError> {
    check_wall(wall)?;
    let pitch = spec.lattice()?.p;
    Ok([pitch / 2.0, (wall.z_lo + wall.z_hi) / 2.0])
}

/// Checks a wall read from outside, returning the sign of its tangent: `1.0` when x grows
/// right-handed about the axis (`axis × x_dir`), `-1.0` when it grows the other way.
fn check_wall(wall: &CylinderWall) -> Result<f64, CellGridError> {
    let bad = |what: &str| CellGridError::Parameter(format!("the wall: {what}"));
    let vectors = [wall.axis_point, wall.axis, wall.x_dir, wall.tangent];
    if !vectors
        .iter()
        .flatten()
        .all(|v| v.is_finite() && v.abs() <= MAX_COORD)
    {
        return Err(bad("a point or direction is not finite"));
    }
    if !(wall.tol.is_finite() && wall.tol > 0.0 && wall.tol < 1.0) {
        return Err(bad("the tolerance must be a small positive length"));
    }
    if !(wall.radius.is_finite() && wall.radius > 0.0 && wall.radius <= MAX_COORD) {
        return Err(bad(
            "the radius must be a positive length up to a billion mm",
        ));
    }
    if !(wall.z_lo.is_finite()
        && wall.z_hi.is_finite()
        && wall.z_lo.abs() <= MAX_COORD
        && wall.z_hi.abs() <= MAX_COORD
        && wall.z_hi - wall.z_lo > wall.tol)
    {
        return Err(bad("its ends must be finite, the upper above the lower"));
    }
    if let Some(f) = wall.facing_radius {
        if !(f.is_finite() && f > 0.0 && f <= MAX_COORD) {
            return Err(bad("the facing radius must be a positive length"));
        }
    }
    let unit = |v: [f64; 3]| (dot(v, v).sqrt() - 1.0).abs() <= 1e-6;
    if !(unit(wall.axis) && unit(wall.x_dir) && dot(wall.axis, wall.x_dir).abs() <= 1e-6) {
        return Err(bad(
            "the axis and the x direction must be unit and square to each other",
        ));
    }
    let along = dot(wall.tangent, cross(wall.axis, wall.x_dir));
    if !((along.abs() - 1.0).abs() <= 1e-6) {
        return Err(bad(
            "the tangent must be square to the axis and the x direction",
        ));
    }
    Ok(along.signum())
}

/// The closed rectangle `[x0, x1] x [y0, y1]`, counter-clockwise.
fn rect_loop(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop2 {
    let pts = [
        Vec2::new(x0, y0),
        Vec2::new(x1, y0),
        Vec2::new(x1, y1),
        Vec2::new(x0, y1),
    ];
    Loop2 {
        segs: pts
            .iter()
            .zip(pts.iter().cycle().skip(1))
            .map(|(&a, &b)| Seg2::Line { a, b })
            .collect(),
    }
}

/// The box `[x0, y0, x1, y1]` of a cell that is one rectangle along the axes: one loop of at least
/// four lines, each along x or y within `tol`, whose area is the box's. `None` otherwise.
fn cell_box(cell: &GridCell, tol: f64) -> Option<[f64; 4]> {
    let [lp] = cell.loops.as_slice() else {
        return None;
    };
    if lp.segs.len() < 4 {
        return None;
    }
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for seg in &lp.segs {
        let Seg2::Line { a, b: e } = *seg else {
            return None;
        };
        if !((a.x - e.x).abs() <= tol || (a.y - e.y).abs() <= tol) {
            return None;
        }
        let [x0, y0, x1, y1] = b;
        b = [
            x0.min(a.x).min(e.x),
            y0.min(a.y).min(e.y),
            x1.max(a.x).max(e.x),
            y1.max(a.y).max(e.y),
        ];
    }
    let [x0, y0, x1, y1] = b;
    let area = (x1 - x0) * (y1 - y0);
    (area.is_finite() && (area - cell.area).abs() <= 1e-9 * area + tol * (x1 - x0 + y1 - y0))
        .then_some(b)
}

/// The row-major placement that turns by `angle` (right-handed) about the line through `q` along
/// the unit `k`, then shifts by `shift` along `k`.
fn placement(q: [f64; 3], k: [f64; 3], angle: f64, shift: f64) -> [[f64; 4]; 4] {
    let [x, y, z] = k;
    let (s, c) = angle.sin_cos();
    let d = 1.0 - c;
    let rows = [
        [c + x * x * d, x * y * d - z * s, x * z * d + y * s],
        [y * x * d + z * s, c + y * y * d, y * z * d - x * s],
        [z * x * d - y * s, z * y * d + x * s, c + z * z * d],
    ];
    let [t0, t1, t2] = add(sub(q, rows.map(|row| dot(row, q))), scale(k, shift));
    let [[a00, a01, a02], [a10, a11, a12], [a20, a21, a22]] = rows;
    [
        [a00, a01, a02, t0],
        [a10, a11, a12, t1],
        [a20, a21, a22, t2],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

fn dot([ax, ay, az]: [f64; 3], [bx, by, bz]: [f64; 3]) -> f64 {
    ax * bx + ay * by + az * bz
}

fn cross([ax, ay, az]: [f64; 3], [bx, by, bz]: [f64; 3]) -> [f64; 3] {
    [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx]
}

fn sub([ax, ay, az]: [f64; 3], [bx, by, bz]: [f64; 3]) -> [f64; 3] {
    [ax - bx, ay - by, az - bz]
}

fn add([ax, ay, az]: [f64; 3], [bx, by, bz]: [f64; 3]) -> [f64; 3] {
    [ax + bx, ay + by, az + bz]
}

fn scale([x, y, z]: [f64; 3], k: f64) -> [f64; 3] {
    [x * k, y * k, z * k]
}
