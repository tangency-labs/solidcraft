//! `solid.cell_grid`: fill the closed profiles of a sketch with cells of one shape and cut them
//! out of the bodies, or with `mode: raised` stand them on the sketch plane and join them (#14044). The cells are planned by solidcore and cut in one operation; the
//! feature is `FeatureKind::CellGrid`, evaluated in `crates/doc/src/eval_cell_grid.rs`.

use serde_json::Value;
use solidcraft_doc::expr::Kind;
use solidcraft_doc::{BodyName, CELL_GRID_MAX_SIDES, CELL_GRID_MIN_SIZE, CellGridMode, Direction, FeatureKind, MAX_CUT_CELLS, SketchId};

use super::CommandSpec;
use super::features::{add_feature, check_expr, default_extrude_profiles, feature_sketch, point_param, profiles, sketch_key};
use crate::params::{bad, bool_, expr, req_expr, str_, string_list};
use crate::schema::cell_grid::CellGridParams;
use crate::schema::features::FeatureResult;
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const CELL_GRID_CONTRACT: TypedContract<CellGridParams> = TypedContract::new();

const HELP_HEAD: &str = "size: expr (a cell's inscribed diameter; not with shape sketch); web: expr (the wall between cells); depth: expr (or through_all: true); shape?: hexagon|square|triangle|circle|polygon|sketch (default hexagon; sketch repeats the one closed profile of the sketch cell_sketch, on a lattice: it needs cell_sketch, pitch_x and pitch_y, ignores size, sides and dot_grid, and clip defaults to whole, snap is refused); cell_sketch?: id|name (shape sketch: the sketch holding the cell, exactly one closed profile without holes); pitch_x?, pitch_y?: expr (shape sketch: the distance between cell origins along a row, and between rows, at least 0.001 mm; a cell that overlaps a neighbour or comes nearer than web is refused); stagger?: expr (shape sketch: how far each row shifts along the row as a fraction of pitch_x, 0 to 1, default 0; 0.5 gives hexagonal and brick layouts); cell_angle?: expr (shape sketch: the cell's turn about its own origin, apart from angle, which turns the whole grid; default 0); flip?: bool (shape sketch: mirror every other cell, those with row plus column odd, in the vertical axis through its origin; default false); cell_origin?: [x,y] (shape sketch: the cell's origin in the cell sketch's own coordinates, the point at each lattice point; default the profile's centroid, anchor is where cell (0,0)'s origin sits); sides?: expr (polygon cells, 3 to 64; default 8); dot_grid?: hexagonal|square (where circle and polygon cells sit; default hexagonal); angle?: expr (the grid's rotation about the anchor; angle 45 deg with square cells gives a diamond layout); anchor?: [x,y,z] (a cell's centre, projected onto the sketch plane; default the sketch origin: grids with the same anchor, angle, size and web line up); clip?: whole|snap|free (cells the field's edge crosses: only whole cells; cells cut along the grid's own lines; cells trimmed to the edge as drawn; default snap, whole with shape sketch); keepout_clip?: whole|snap|free (the same for the edges of loops inside the field, the keep-outs; default: the value of clip, so one mode serves every loop; a cell that meets both kinds is dropped if either is whole, else trimmed if either is free); snap_inset?: expr (with clip snap, a whole number of lattice lines each straight field edge moves in after it snaps, so the edge cells are whole and the rim thicker by whole grid rows; default 0; keep-outs are not moved); margin?: expr (with clip whole, the least clearance from a cell to every edge of the field and its keep-outs, measured exactly; default 0, a cell may touch an edge); direction?: positive|negative|symmetric (default positive); sketch?: id|name (default: active or last sketch; its closed profiles hold the field: a loop inside another is a keep-out); profiles?: as solid.extrude; targets?: [body]; mode?: cut|raised (default cut; raised adds the cells as material standing `depth` from the sketch plane in `direction`, joined to the targets, or a new body if there is none; not with through_all; the sketch plane is where they stand, so the sketch on the face they rise from and direction away from the body); name?. Cuts or raises at most ";
const HELP_TAIL: &str = " cells in one operation; not for sheet metal (use sheet.perforate). The faces cut have no stable names yet";
const HELP_LEN: usize = HELP_HEAD.len() + decimal_len(MAX_CUT_CELLS) + HELP_TAIL.len();
const HELP_BYTES: [u8; HELP_LEN] = help_bytes();
/// The parameter help, with the cap read from [`MAX_CUT_CELLS`] so the text cannot drift from the limit.
const HELP: &str = match std::str::from_utf8(&HELP_BYTES) {
    Ok(h) => h,
    Err(_) => HELP_HEAD,
};

const fn decimal_len(mut n: usize) -> usize {
    let mut len = 1;
    while n >= 10 {
        n /= 10;
        len += 1;
    }
    len
}

const fn help_bytes() -> [u8; HELP_LEN] {
    let mut out = [0u8; HELP_LEN];
    let mut at = 0;
    let mut i = 0;
    while i < HELP_HEAD.len() {
        out[at] = HELP_HEAD.as_bytes()[i];
        at += 1;
        i += 1;
    }
    let mut n = MAX_CUT_CELLS;
    let mut d = decimal_len(n);
    while d > 0 {
        d -= 1;
        out[at + d] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    at += decimal_len(MAX_CUT_CELLS);
    let mut i = 0;
    while i < HELP_TAIL.len() {
        out[at] = HELP_TAIL.as_bytes()[i];
        at += 1;
        i += 1;
    }
    out
}

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("solid.cell_grid", "Cell Grid", crate::checked!("solid.cell_grid", cell_grid))
    .begins_edit()
    .icon("perforate")
    .contract(&CELL_GRID_CONTRACT)
    .output(output_schema_for::<FeatureResult>)
    .params(HELP)];

/// A length parameter that must be a usable cell size or web: finite and at least [`CELL_GRID_MIN_SIZE`].
fn size_param(s: &Session, p: &Value, key: &str, cmd: &str) -> Result<solidcraft_doc::Expr> {
    let e = req_expr(cmd, p, key)?;
    check_expr(s, &e, Kind::Length, cmd, key)?;
    // A bare number means the design's unit: the minimum is checked on the length it will be.
    let typed: solidcraft_doc::Expr = s.doc.with_design_unit(&e, Kind::Length).into();
    let v = s.doc.eval(&typed, Kind::Length).map_err(|err| bad(cmd, format!("{key}: {err}")))?;
    if !(v.is_finite() && v >= CELL_GRID_MIN_SIZE) {
        return Err(bad(cmd, format!("`{key}` must be at least {CELL_GRID_MIN_SIZE} mm, not {v} mm")));
    }
    Ok(e)
}

fn keyword<T: serde::de::DeserializeOwned>(p: &Value, key: &str, cmd: &str, default: T, allowed: &str) -> Result<T> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| bad(cmd, format!("`{key}` must be {allowed}"))),
    }
}

/// The sketched cell's parameters of a `shape: sketch` grid, as the feature stores them.
struct Sketched {
    sketch: SketchId,
    pitch_x: solidcraft_doc::Expr,
    pitch_y: solidcraft_doc::Expr,
    stagger: Option<solidcraft_doc::Expr>,
    cell_angle: Option<solidcraft_doc::Expr>,
    flip: bool,
    origin: Option<(solidcraft_doc::Expr, solidcraft_doc::Expr)>,
}

/// The parameters only a sketched cell has, refused when the shape is a built-in one.
fn refuse_sketch_params(p: &Value, cmd: &str) -> Result<()> {
    for key in ["cell_sketch", "pitch_x", "pitch_y", "stagger", "cell_angle", "flip", "cell_origin"] {
        if p.get(key).is_some_and(|v| !v.is_null()) {
            return Err(bad(cmd, format!("`{key}` is for `shape: sketch`; this grid's shape is a built-in one")));
        }
    }
    Ok(())
}

/// The sketched cell of a `shape: sketch` grid: its sketch (exactly one closed profile, checked on
/// the solved sketch) and its layout, every number checked finite and in range.
fn sketched(s: &Session, p: &Value, cmd: &str) -> Result<Sketched> {
    let key = match p.get("cell_sketch").filter(|v| !v.is_null()) {
        None => return Err(bad(cmd, "`shape: sketch` needs `cell_sketch`, the sketch holding the cell")),
        Some(v) => sketch_key(Some(v)).ok_or_else(|| bad(cmd, "`cell_sketch` must be an id or name"))?,
    };
    let f = s.doc.find_feature(&key).ok_or_else(|| bad(cmd, format!("no cell sketch `{key}`")))?;
    if !matches!(f.kind, FeatureKind::Sketch { .. }) {
        return Err(bad(cmd, format!("`{key}` is not a sketch")));
    }
    let id = f.id;
    let st = s.model.state();
    let solved =
        st.sketch(id).ok_or_else(|| bad(cmd, format!("the cell sketch `{key}` has not been built (it must come earlier in the timeline)")))?;
    solidcraft_doc::cell_profile(solved).map_err(|e| bad(cmd, e.to_string()))?;
    let pitch_x = size_param(s, p, "pitch_x", cmd)?;
    let pitch_y = size_param(s, p, "pitch_y", cmd)?;
    let stagger = match expr(p, "stagger") {
        Some(e) => {
            check_expr(s, &e, Kind::Unitless, cmd, "stagger")?;
            let v = s.doc.eval(&e, Kind::Unitless).map_err(|err| bad(cmd, format!("stagger: {err}")))?;
            if !(v.is_finite() && (0.0..=1.0).contains(&v)) {
                return Err(bad(cmd, format!("`stagger` must be from 0 to 1, not {v}")));
            }
            Some(e)
        }
        None if p.get("stagger").is_some_and(|v| !v.is_null()) => return Err(bad(cmd, "`stagger` must be a number or an expression")),
        None => None,
    };
    let cell_angle = match expr(p, "cell_angle") {
        Some(e) => {
            check_expr(s, &e, Kind::Angle, cmd, "cell_angle")?;
            let v = s.doc.eval(&e, Kind::Angle).map_err(|err| bad(cmd, format!("cell_angle: {err}")))?;
            if !v.is_finite() {
                return Err(bad(cmd, "`cell_angle` must be a finite angle"));
            }
            Some(e)
        }
        None if p.get("cell_angle").is_some_and(|v| !v.is_null()) => return Err(bad(cmd, "`cell_angle` must be an angle or an expression")),
        None => None,
    };
    let flip = match p.get("flip") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad(cmd, "`flip` must be true or false")),
    };
    let origin = match p.get("cell_origin").filter(|v| !v.is_null()) {
        None => None,
        Some(v) => {
            let want = || bad(cmd, "`cell_origin` must be a point [x, y] of numbers or length expressions");
            let coords: Vec<&Value> = match v {
                Value::Array(a) if a.len() == 2 => a.iter().collect(),
                Value::Object(o) => ["x", "y"].iter().map(|k| o.get(*k)).collect::<Option<Vec<_>>>().ok_or_else(want)?,
                _ => return Err(want()),
            };
            let mut out = Vec::with_capacity(2);
            for (c, axis) in coords.into_iter().zip(["x", "y"]) {
                let e = expr(&serde_json::json!({ "v": c }), "v").ok_or_else(want)?;
                let typed: solidcraft_doc::Expr = s.doc.with_design_unit(&e, Kind::Length).into();
                let x = s.doc.eval(&typed, Kind::Length).map_err(|err| bad(cmd, format!("cell_origin {axis}: {err}")))?;
                if !(x.is_finite() && x.abs() < 1e9) {
                    return Err(bad(cmd, format!("cell_origin {axis} is out of range")));
                }
                out.push(e);
            }
            match <[solidcraft_doc::Expr; 2]>::try_from(out) {
                Ok([x, y]) => Some((x, y)),
                Err(_) => return Err(want()),
            }
        }
    };
    Ok(Sketched { sketch: SketchId(id), pitch_x, pitch_y, stagger, cell_angle, flip, origin })
}

fn cell_grid(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.cell_grid";
    let sketch = feature_sketch(s, p, cmd)?;
    let mode: CellGridMode = keyword(p, "mode", cmd, CellGridMode::Cut, "cut or raised")?;
    // `sketch` is not one of the record's shapes: a sketched cell is the feature's `cell_sketch`,
    // and its `shape` stays the default, unused.
    let is_sketch = p.get("shape").and_then(Value::as_str) == Some("sketch");
    let shape = if is_sketch {
        solidcore_record::cell_grid::CellShape::Hexagon
    } else {
        keyword(p, "shape", cmd, solidcore_record::cell_grid::CellShape::Hexagon, "hexagon, square, triangle, circle, polygon or sketch")?
    };
    let cell = if is_sketch {
        Some(sketched(s, p, cmd)?)
    } else {
        refuse_sketch_params(p, cmd)?;
        None
    };
    let dot_grid = if is_sketch {
        solidcore_record::cell_grid::DotGrid::Hexagonal
    } else {
        keyword(p, "dot_grid", cmd, solidcore_record::cell_grid::DotGrid::Hexagonal, "hexagonal or square")?
    };
    // A sketched cell has no chords of its own to snap to: its grid is whole unless asked.
    let clip_default = if is_sketch { solidcore_record::cell_grid::Clip::Whole } else { solidcore_record::cell_grid::Clip::Snap };
    let clip = keyword(p, "clip", cmd, clip_default, "whole, snap or free")?;
    let keepout_clip = match p.get("keepout_clip").filter(|v| !v.is_null()) {
        None => None,
        Some(_) => Some(keyword(p, "keepout_clip", cmd, clip, "whole, snap or free")?),
    };
    if is_sketch && (clip == solidcore_record::cell_grid::Clip::Snap || keepout_clip == Some(solidcore_record::cell_grid::Clip::Snap)) {
        return Err(bad(cmd, "`clip: snap` is not for a sketched cell: snap needs the cell's own chords; use whole or free"));
    }
    let snap_inset = expr(p, "snap_inset");
    if let Some(e) = &snap_inset {
        check_expr(s, e, Kind::Unitless, cmd, "snap_inset")?;
        let n = s.doc.eval(e, Kind::Unitless).map_err(|err| bad(cmd, format!("snap_inset: {err}")))?;
        let most = f64::from(solidcraft_doc::CELL_GRID_MAX_SNAP_INSET);
        if !(n.is_finite() && n.fract() == 0.0 && (0.0..=most).contains(&n)) {
            return Err(bad(cmd, format!("`snap_inset` must be a whole number from 0 to {most}, not {n}")));
        }
        if n > 0.0 && clip != solidcore_record::cell_grid::Clip::Snap {
            return Err(bad(cmd, "`snap_inset` needs `clip: snap`"));
        }
    } else if p.get("snap_inset").is_some_and(|v| !v.is_null()) {
        return Err(bad(cmd, "`snap_inset` must be a number or an expression"));
    }
    // `sides` and `size` are not used by a sketched cell, which is given its outline and pitch.
    let sides = if is_sketch { None } else { expr(p, "sides") };
    if !is_sketch {
        if let Some(e) = &sides {
            check_expr(s, e, Kind::Unitless, cmd, "sides")?;
            let n = s.doc.eval(e, Kind::Unitless).map_err(|err| bad(cmd, format!("sides: {err}")))?;
            let most = f64::from(CELL_GRID_MAX_SIDES);
            if !(n.is_finite() && n.fract() == 0.0 && (3.0..=most).contains(&n)) {
                return Err(bad(cmd, format!("`sides` must be a whole number from 3 to {most}, not {n}")));
            }
        } else if p.get("sides").is_some_and(|v| !v.is_null()) {
            return Err(bad(cmd, "`sides` must be a number or an expression"));
        }
    }
    let size = if is_sketch { None } else { Some(size_param(s, p, "size", cmd)?) };
    let web = size_param(s, p, "web", cmd)?;
    let angle = expr(p, "angle");
    if let Some(a) = &angle {
        check_expr(s, a, Kind::Angle, cmd, "angle")?;
    }
    let anchor = point_param(s, p, "anchor", cmd)?;
    let margin = match expr(p, "margin") {
        Some(e) => {
            check_expr(s, &e, Kind::Length, cmd, "margin")?;
            // A bare number means the design's unit.
            let typed: solidcraft_doc::Expr = s.doc.with_design_unit(&e, Kind::Length).into();
            let v = s.doc.eval(&typed, Kind::Length).map_err(|err| bad(cmd, format!("margin: {err}")))?;
            if !(v.is_finite() && v >= 0.0) {
                return Err(bad(cmd, format!("`margin` must be 0 or more, not {v} mm")));
            }
            if v > 0.0
                && (clip != solidcore_record::cell_grid::Clip::Whole || keepout_clip.is_some_and(|c| c != solidcore_record::cell_grid::Clip::Whole))
            {
                return Err(bad(
                    cmd,
                    "`margin` needs `clip: whole` (and `keepout_clip: whole` if given): snapped and trimmed cells meet the edge by design",
                ));
            }
            Some(e)
        }
        None if p.get("margin").is_some_and(|v| !v.is_null()) => return Err(bad(cmd, "`margin` must be a number or an expression")),
        None => None,
    };
    let through_all = bool_(p, "through_all").unwrap_or(false);
    if through_all && mode == CellGridMode::Raised {
        return Err(bad(cmd, "`through_all` cuts through bodies: a raised grid stands `depth` high, so give `depth`"));
    }
    let depth = if through_all {
        None
    } else {
        let d = req_expr(cmd, p, "depth").map_err(|_| bad(cmd, "give `depth` or `through_all: true`"))?;
        check_expr(s, &d, Kind::Length, cmd, "depth")?;
        let v = s.doc.eval(&d, Kind::Length).map_err(|err| bad(cmd, format!("depth: {err}")))?;
        if !(v.is_finite() && v > 0.0) {
            return Err(bad(cmd, format!("`depth` must be more than 0, not {v}")));
        }
        Some(d)
    };
    let direction = match str_(p, "direction").map(str::to_ascii_lowercase).as_deref() {
        None | Some("positive") | Some("one_side") => Direction::Positive,
        Some("negative") | Some("reverse") | Some("flip") => Direction::Negative,
        Some("symmetric") => Direction::Symmetric,
        Some(o) => return Err(bad(cmd, format!("unknown direction `{o}`"))),
    };
    let targets = string_list(cmd, p, "targets")?;
    let profiles = if p.get("profiles").is_none() { default_extrude_profiles(s, sketch) } else { profiles(p, cmd)? };
    add_feature(
        s,
        p,
        FeatureKind::CellGrid {
            sketch: SketchId(sketch),
            profiles,
            mode,
            shape,
            sides,
            dot_grid,
            size,
            cell_sketch: cell.as_ref().map(|c| c.sketch),
            pitch_x: cell.as_ref().map(|c| c.pitch_x.clone()),
            pitch_y: cell.as_ref().map(|c| c.pitch_y.clone()),
            stagger: cell.as_ref().and_then(|c| c.stagger.clone()),
            cell_angle: cell.as_ref().and_then(|c| c.cell_angle.clone()),
            flip: cell.as_ref().is_some_and(|c| c.flip),
            cell_origin_x: cell.as_ref().and_then(|c| c.origin.as_ref().map(|o| o.0.clone())),
            cell_origin_y: cell.as_ref().and_then(|c| c.origin.as_ref().map(|o| o.1.clone())),
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
            targets: targets.into_iter().map(BodyName).collect(),
        },
    )
}

#[cfg(test)]
#[path = "cell_grid_tests.rs"]
mod tests;
