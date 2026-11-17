//! `solid.living_hinge`: cut rows of staggered slots through a plate inside the closed profiles of
//! a sketch, so the plate can bend there (#17484). The slots are planned by solidcore and cut in one
//! operation; the feature is `FeatureKind::LivingHinge`, evaluated in
//! `crates/doc/src/eval_living_hinge.rs`.

use serde_json::Value;
use solidcore_record::Loop2;
use solidcore_record::living_hinge::HingeClip;
use solidcraft_doc::expr::Kind;
use solidcraft_doc::{BodyName, Expr, FeatureKind, MAX_CUT_SLOTS, ProfileSel, SketchId};

use super::CommandSpec;
use super::features::{add_feature, check_expr, default_extrude_profiles, feature_sketch, point_param, profiles};
use super::help_text::help_with_cap;
use crate::params::{bad, expr, req_expr, string_list};
use crate::schema::living_hinge::{LivingHingeParams, LivingHingeResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const LIVING_HINGE_CONTRACT: TypedContract<LivingHingeParams> = TypedContract::new();

/// The least length of a slot's straight part, a bridge, the gap between rows or a slot's width (mm).
const MIN_SIZE: f64 = 0.001;
/// The largest length the command takes (mm).
const MAX_SIZE: f64 = 1e9;
/// A slot's width when none is given (mm).
const DEFAULT_SLOT_WIDTH: f64 = 0.2;

const HELP_HEAD: &str = "slot_length: expr (a slot's length tip to tip, as cut); bridge: expr (the material between two slots in one row); row_spacing: expr (centre line to centre line between rows); slot_width?: expr (the cut's width, about a laser's kerf; default 0.2 mm); angle?: expr (the slots' direction, which is the bend axis, from the sketch's X axis; default 0); anchor?: [x,y,z] (a slot's centre, projected onto the sketch plane; default: the pattern centred on the area); clip?: run_out|shorten|whole (slots the edges cross: every other row runs out through the plate's edge and the rest are shortened; all shortened, leaving a solid border; or only whole slots; default run_out); margin?: expr (the least material between a slot's end and every edge of the area and its keep-outs; default 0); sketch?: id|name (default: active or last sketch; its closed profiles hold the area: a loop inside another is a keep-out); profiles?: as solid.extrude; targets?: [body] (the plate; default: the one body with a face in the sketch's plane under the area, found when the feature is made and stored in it); name?. Cuts through the plate, at most ";
const HELP_TAIL: &str =
    " slots in one operation (more are refused with their count). Not for sheet metal yet. The faces cut have no stable names yet";
/// The parameter help, with the cap read from [`MAX_CUT_SLOTS`] so the text cannot drift from the limit.
const HELP: &str = help_with_cap!(HELP_HEAD, MAX_CUT_SLOTS, HELP_TAIL);

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("solid.living_hinge", "Living Hinge", crate::checked!("solid.living_hinge", living_hinge))
    .icon("perforate")
    .contract(&LIVING_HINGE_CONTRACT)
    .output(output_schema_for::<LivingHingeResult>)
    .params(HELP)];

/// A length's value now, in mm (a bare number means the design's unit).
fn length_now(s: &Session, e: &Expr, key: &str, cmd: &str) -> Result<f64> {
    check_expr(s, e, Kind::Length, cmd, key)?;
    let typed: Expr = s.doc.with_design_unit(e, Kind::Length).into();
    let v = s.doc.eval(&typed, Kind::Length).map_err(|err| bad(cmd, format!("{key}: {err}")))?;
    if !v.is_finite() || v.abs() > MAX_SIZE {
        return Err(bad(cmd, format!("`{key}` must be a finite length of at most {MAX_SIZE} mm, not {v} mm")));
    }
    Ok(v)
}

fn optional_expr(p: &Value, key: &str, cmd: &str) -> Result<Option<Expr>> {
    match expr(p, key) {
        Some(e) => Ok(Some(e)),
        None if p.get(key).is_some_and(|v| !v.is_null()) => Err(bad(cmd, format!("`{key}` must be a number or an expression"))),
        None => Ok(None),
    }
}

fn living_hinge(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.living_hinge";
    let sketch = feature_sketch(s, p, cmd)?;
    let clip = match p.get("clip").filter(|v| !v.is_null()) {
        None => HingeClip::RunOut,
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| bad(cmd, "`clip` must be run_out, shorten or whole"))?,
    };
    let slot_width = optional_expr(p, "slot_width", cmd)?;
    let width = match &slot_width {
        Some(e) => length_now(s, e, "slot_width", cmd)?,
        None => DEFAULT_SLOT_WIDTH,
    };
    if width < MIN_SIZE {
        return Err(bad(cmd, format!("`slot_width` must be at least {MIN_SIZE} mm, not {width} mm")));
    }
    let slot_length = req_expr(cmd, p, "slot_length")?;
    let length = length_now(s, &slot_length, "slot_length", cmd)?;
    if length - width < MIN_SIZE {
        return Err(bad(cmd, format!("`slot_length` must be longer than the slot width ({width} mm) by at least {MIN_SIZE} mm, not {length} mm")));
    }
    let bridge = req_expr(cmd, p, "bridge")?;
    let b = length_now(s, &bridge, "bridge", cmd)?;
    if b < MIN_SIZE {
        return Err(bad(cmd, format!("`bridge` must be at least {MIN_SIZE} mm, not {b} mm")));
    }
    let row_spacing = req_expr(cmd, p, "row_spacing")?;
    let spacing = length_now(s, &row_spacing, "row_spacing", cmd)?;
    if spacing - width < MIN_SIZE {
        return Err(bad(cmd, format!("`row_spacing` must be more than the slot width ({width} mm) by at least {MIN_SIZE} mm, not {spacing} mm")));
    }
    let angle = optional_expr(p, "angle", cmd)?;
    if let Some(a) = &angle {
        check_expr(s, a, Kind::Angle, cmd, "angle")?;
    }
    let margin = optional_expr(p, "margin", cmd)?;
    if let Some(m) = &margin
        && length_now(s, m, "margin", cmd)? < 0.0
    {
        return Err(bad(cmd, "`margin` must be 0 or more"));
    }
    let anchor = point_param(s, p, "anchor", cmd)?;
    let profiles = if p.get("profiles").is_none() { default_extrude_profiles(s, sketch) } else { profiles(p, cmd)? };
    let mut targets = string_list(cmd, p, "targets")?;
    if targets.is_empty() {
        targets = resolve_plate(s, sketch, &profiles, cmd)?;
    }
    let mut reply = add_feature(
        s,
        p,
        FeatureKind::LivingHinge {
            sketch: SketchId(sketch),
            profiles,
            slot_length,
            bridge,
            row_spacing,
            slot_width,
            angle,
            anchor,
            clip,
            margin,
            targets: targets.into_iter().map(BodyName).collect(),
        },
    )?;
    with_report(s, &mut reply);
    Ok(reply)
}

/// The default plate (design 2, 3.5): the one body with a flat face in the sketch's plane whose
/// outline meets the area, read from the model as it stands before the new feature. Every decision
/// is a kernel call; this only counts the answers. None or several are refused.
fn resolve_plate(s: &Session, sketch: u64, profiles: &ProfileSel, cmd: &str) -> Result<Vec<String>> {
    let st = s.model.state();
    let Some(ss) = st.sketch(sketch) else { return Ok(Vec::new()) };
    let regions = solidcraft_sketch::merge_regions(&solidcraft_doc::select_profiles(ss, profiles).map_err(|e| bad(cmd, e.to_string()))?);
    let field: Vec<Loop2> = regions.iter().flat_map(|r| r.loops().cloned()).collect();
    let mut found = Vec::new();
    for b in &st.bodies {
        // A body the plate reader cannot take (a mesh) is no plate.
        let Ok(outline) = solidcraft_kernel::living_hinge::plate_outline(&b.body, &ss.plane) else { continue };
        for face in outline.faces.iter().filter(|f| f.in_plane) {
            let loops: Vec<Loop2> = face.regions.iter().flat_map(|r| r.loops().cloned()).collect();
            if solidcraft_kernel::living_hinge::meets(&field, &loops).map_err(|e| bad(cmd, e.to_string()))? {
                found.push(b.name.clone());
                break;
            }
        }
    }
    match found.len() {
        1 => Ok(found),
        0 => Err(bad(
            cmd,
            "no plate has a flat face in the sketch's plane under the area: draw the area on the plate's face, or name the plate with \"targets\"",
        )),
        _ => {
            let last = found.pop().unwrap_or_default();
            Err(bad(cmd, format!("the area lies over {} and {last}: name the plate to cut with \"targets\"", found.join(", "))))
        }
    }
}

/// Adds the feature's report (what the evaluation cut) to its reply.
fn with_report(s: &Session, reply: &mut Value) {
    let Some(id) = reply.get("feature").and_then(Value::as_u64) else { return };
    let st = s.model.state();
    let Some(r) = st.living_hinge_report(id) else { return };
    let Some(o) = reply.as_object_mut() else { return };
    for (k, v) in [
        ("slots", Value::from(r.slots)),
        ("rows", r.rows.into()),
        ("whole", r.whole.into()),
        ("shortened", r.shortened.into()),
        ("run_out", r.run_out.into()),
        ("left_out", r.left_out.into()),
        ("cut_length_mm", r.cut_length.into()),
        ("outline_length_mm", r.outline_length.into()),
        ("open_area_mm2", r.open_area.into()),
        ("band_width_mm", r.band_width.into()),
    ] {
        o.insert(k.into(), v);
    }
}

#[cfg(test)]
#[path = "living_hinge_tests.rs"]
mod tests;
