//! `solid.cell_grid`: fill the closed profiles of a sketch with cells of one shape and cut them
//! out of the bodies (#14044). The cells are planned by solidcore and cut in one operation; the
//! feature is `FeatureKind::CellGrid`, evaluated in `crates/doc/src/eval_cell_grid.rs`.

use serde_json::Value;
use solidcraft_doc::expr::Kind;
use solidcraft_doc::{BodyName, CELL_GRID_MAX_SIDES, CELL_GRID_MIN_SIZE, CellGridMode, Direction, FeatureKind, MAX_CUT_CELLS, SketchId};

use super::CommandSpec;
use super::features::{add_feature, check_expr, default_extrude_profiles, feature_sketch, point_param, profiles};
use crate::params::{bad, bool_, expr, req_expr, str_, string_list};
use crate::schema::cell_grid::CellGridParams;
use crate::schema::features::FeatureResult;
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const CELL_GRID_CONTRACT: TypedContract<CellGridParams> = TypedContract::new();

const HELP_HEAD: &str = "size: expr (a cell's inscribed diameter); web: expr (the wall between cells); depth: expr (or through_all: true); shape?: hexagon|square|triangle|circle|polygon (default hexagon); sides?: expr (polygon cells, 3 to 64; default 8); dot_grid?: hexagonal|square (where circle and polygon cells sit; default hexagonal); angle?: expr (the grid's rotation about the anchor; angle 45 deg with square cells gives a diamond layout); anchor?: [x,y,z] (a cell's centre, projected onto the sketch plane; default the sketch origin: grids with the same anchor, angle, size and web line up); clip?: whole|snap|free (cells the field's edge crosses: only whole cells; cells cut along the grid's own lines; cells trimmed to the edge as drawn; default snap); direction?: positive|negative|symmetric (default positive); sketch?: id|name (default: active or last sketch; its closed profiles hold the field: a loop inside another is a keep-out); profiles?: as solid.extrude; targets?: [body]; mode?: cut (the only mode so far); name?. Cuts at most ";
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

fn cell_grid(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.cell_grid";
    let sketch = feature_sketch(s, p, cmd)?;
    let mode: CellGridMode = keyword(p, "mode", cmd, CellGridMode::Cut, "cut (the only mode so far)")?;
    let shape = keyword(p, "shape", cmd, solidcore_record::cell_grid::CellShape::Hexagon, "hexagon, square, triangle, circle or polygon")?;
    let dot_grid = keyword(p, "dot_grid", cmd, solidcore_record::cell_grid::DotGrid::Hexagonal, "hexagonal or square")?;
    let clip = keyword(p, "clip", cmd, solidcore_record::cell_grid::Clip::Snap, "whole, snap or free")?;
    let sides = expr(p, "sides");
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
    let size = size_param(s, p, "size", cmd)?;
    let web = size_param(s, p, "web", cmd)?;
    let angle = expr(p, "angle");
    if let Some(a) = &angle {
        check_expr(s, a, Kind::Angle, cmd, "angle")?;
    }
    let anchor = point_param(s, p, "anchor", cmd)?;
    let through_all = bool_(p, "through_all").unwrap_or(false);
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
            web,
            angle,
            anchor,
            clip,
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
