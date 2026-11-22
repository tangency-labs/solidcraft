//! Stretch (#20512): a solid made longer or shorter along one of its component's axes, its
//! details keeping their size. The geometry is `kernel::stretch`; the feature is
//! `FeatureKind::Stretch`.

use serde_json::Value;
use solidcraft_doc::expr::Kind;
use solidcraft_doc::{FeatureKind, StretchDirection};
use solidcraft_geom::prelude::*;

use super::{
    CommandSpec,
    features::{add_feature, check_expr},
};
use crate::schema::features::FeatureResult;
use crate::schema::stretch::StretchParams;
use crate::schema::{TypedContract, output_schema_for};
use crate::{
    Result, Sel, Session,
    params::{bad, expr, str_, vec3},
};

const STRETCH_CONTRACT: TypedContract<StretchParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("solid.stretch", "Stretch", crate::checked!("solid.stretch", stretch))
    .at("SOLID", "MODIFY")
    .icon("move")
    .contract(&STRETCH_CONTRACT)
    .output(output_schema_for::<FeatureResult>)
    .params("body?: solid name (default: the selected body); direction: +x|-x|+y|-y|+z|-z (the way the moving side goes, in the body's component; the opposite side stays put); from?: [x,y,z] (a point on a face the cut is measured from; default: the side that stays put); at?: length expr (the cut plane's distance along the direction from `from`, or from the side that stays put; default: half the body's size along the direction, or 0 past `from`); distance: length expr (positive stretches, negative shrinks by taking a slice out): the body is cut square to the direction and the part beyond moves, the gap filled with the body's own section, so holes, rounds and walls keep their size. Refused, naming the face, where the body is not the same shape all along the direction at the cut; a refused shrink says how far it can go. A Stretch feature in the body's component; name?")];

/// The body to stretch: `body`, else the one selected body.
fn target(s: &Session, p: &Value, cmd: &str) -> Result<String> {
    if let Some(name) = str_(p, "body").filter(|b| !b.trim().is_empty()) {
        return Ok(name.to_owned());
    }
    let selected: Vec<&str> = s.selection.iter().filter_map(|x| if let Sel::Body { name } = x { Some(name.as_str()) } else { None }).collect();
    match selected.as_slice() {
        [name] => Ok((*name).to_owned()),
        [] => Err(bad(cmd, "select a body to stretch")),
        _ => Err(bad(cmd, "Stretch takes one body at a time")),
    }
}

fn stretch(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.stretch";
    let given = target(s, p, cmd)?;
    let state = s.model.state();
    // A placed copy's name means the body it shows (#9740); the stretch goes on that body.
    let body = s.own_body_name(&given).ok_or_else(|| bad(cmd, format!("no body `{given}`")))?;
    let source = state.body(&body).ok_or_else(|| bad(cmd, format!("no body `{given}`")))?;
    if source.body.is_mesh() {
        return Err(bad(cmd, "Stretch takes a solid, not a mesh body: convert the mesh first"));
    }
    let direction =
        str_(p, "direction").and_then(StretchDirection::parse).ok_or_else(|| bad(cmd, "`direction` must be one of +x, -x, +y, -y, +z, -z"))?;
    let distance = expr(p, "distance").ok_or_else(|| bad(cmd, "`distance` must be a length"))?;
    check_expr(s, &distance, Kind::Length, cmd, "distance")?;
    let from = match p.get("from").filter(|v| !v.is_null()) {
        Some(v) => Some(vec3(v).filter(|p| p.is_finite()).ok_or_else(|| bad(cmd, "`from` must be a point [x, y, z] on a face"))?),
        None => None,
    };
    let at = match expr(p, "at") {
        Some(at) => at,
        None if from.is_some() => "0 mm".into(),
        None => {
            let n = direction.vector();
            let b = source.body.exact_bounds().map_err(|e| bad(cmd, e.to_string()))?;
            let size = (b.max.dot(n) - b.min.dot(n)).abs();
            format!("{} mm", (size / 2.0 * 1e6).round() / 1e6).into()
        }
    };
    check_expr(s, &at, Kind::Length, cmd, "at")?;
    let owner = s.doc.model_body_component(source);
    if !s.doc.component_within(owner, s.active_component) {
        let name = s.doc.components.iter().find(|c| c.id == owner).map(|c| c.name.as_str()).unwrap_or(s.doc.name.as_str());
        return Err(bad(cmd, format!("activate component '{name}' first")));
    }
    // The stretch edits the component definition, independent of its occurrence placement.
    let active_component = std::mem::replace(&mut s.active_component, owner);
    let active_occurrence_path = s.active_occurrence_path.take();
    let result = add_feature(s, p, FeatureKind::Stretch { body: body.into(), direction, from: from.map(solidcraft_doc::FacePick), at, distance });
    s.active_component = active_component;
    s.active_occurrence_path = active_occurrence_path;
    result
}

#[cfg(test)]
#[path = "stretch_tests.rs"]
mod tests;
