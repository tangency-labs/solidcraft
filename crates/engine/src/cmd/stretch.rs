//! Stretch (#20512): a solid made longer or shorter along one of its component's axes, its
//! details keeping their size. The geometry is `kernel::stretch`; the feature is
//! `FeatureKind::Stretch`.

use serde_json::{Value, json};
use solidcraft_doc::expr::Kind;
use solidcraft_doc::{AxisStretch, FeatureKind, StretchDirection};
use solidcraft_geom::prelude::*;

use super::stretch_sketch;
use super::{
    CommandSpec,
    features::{add_feature, check_expr},
};
use crate::schema::stretch::{StretchParams, StretchResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{
    Result, Sel, Session,
    params::{bad, bool_, expr, str_, vec3},
};

const STRETCH_CONTRACT: TypedContract<StretchParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("solid.stretch", "Stretch", crate::checked!("solid.stretch", stretch))
    .at("SOLID", "MODIFY")
    .icon("move")
    .contract(&STRETCH_CONTRACT)
    .output(output_schema_for::<StretchResult>)
    .params("body?: solid name (default: the selected body); stretches?: [{direction, from?, at?, distance}] (one to three, each along a different axis, applied in order; instead of the one stretch given by the fields below); direction: +x|-x|+y|-y|+z|-z (the way the moving side goes, in the body's component; the opposite side stays put); from?: [x,y,z] (a point on a face the cut is measured from; default: the side that stays put); at?: length expr (the cut plane's distance along the direction from `from`, or from the side that stays put; default: half the body's size along the direction, or 0 past `from`); distance: length expr (positive stretches, negative shrinks by taking a slice out): the body is cut square to the direction and the part beyond moves, the gap filled with the body's own section, so holes, rounds and walls keep their size. Refused, naming the face, where the body is not the same shape all along the direction at the cut; a refused shrink says how far it can go. A Stretch feature in the body's component; edit_sketch?: bool (default false: when the body was made by an Extrude of a sketch, grow the sketch's dimensions that span each cut, or the Extrude's depth, instead of adding a Stretch step, if the part they rebuild is the stretched one; otherwise a Stretch step, the reply's sketch_not_edited saying why); name?")];

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
    // One stretch from the top-level fields, or up to three, one per axis, from `stretches`.
    let given: Vec<&Value> = match p.get("stretches").filter(|v| !v.is_null()) {
        Some(Value::Array(list)) => list.iter().collect(),
        Some(_) => return Err(bad(cmd, "`stretches` must be a list")),
        None => vec![p],
    };
    if given.is_empty() || given.len() > 3 {
        return Err(bad(cmd, "Stretch takes one to three stretches, one per axis"));
    }
    let mut stretches: Vec<AxisStretch> = Vec::new();
    for one in given {
        let direction =
            str_(one, "direction").and_then(StretchDirection::parse).ok_or_else(|| bad(cmd, "`direction` must be one of +x, -x, +y, -y, +z, -z"))?;
        if stretches.iter().any(|s| s.direction.axis() == direction.axis()) {
            return Err(bad(cmd, "each stretch goes along a different axis"));
        }
        let distance = expr(one, "distance").ok_or_else(|| bad(cmd, "`distance` must be a length"))?;
        check_expr(s, &distance, Kind::Length, cmd, "distance")?;
        let from = match one.get("from").filter(|v| !v.is_null()) {
            Some(v) => Some(vec3(v).filter(|p| p.is_finite()).ok_or_else(|| bad(cmd, "`from` must be a point [x, y, z] on a face"))?),
            None => None,
        };
        let at = match expr(one, "at") {
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
        stretches.push(AxisStretch { direction, from: from.map(solidcraft_doc::FacePick), at, distance });
    }
    let owner = s.doc.model_body_component(source);
    if !s.doc.component_within(owner, s.active_component) {
        let name = s.doc.components.iter().find(|c| c.id == owner).map(|c| c.name.as_str()).unwrap_or(s.doc.name.as_str());
        return Err(bad(cmd, format!("activate component '{name}' first")));
    }
    // With `edit_sketch`, the sketch and Extrude that made the body change instead, when they
    // can give the same part; otherwise, and without it, a Stretch step.
    let mut not_edited = None;
    if bool_(p, "edit_sketch").unwrap_or(false) {
        match edit_sketch(s, &body, &stretches) {
            Ok(reply) => return Ok(reply),
            Err(why) => not_edited = Some(why),
        }
    }
    // The stretch edits the component definition, independent of its occurrence placement.
    let active_component = std::mem::replace(&mut s.active_component, owner);
    let active_occurrence_path = s.active_occurrence_path.take();
    let result = add_feature(s, p, FeatureKind::Stretch { body: body.into(), stretches });
    s.active_component = active_component;
    s.active_occurrence_path = active_occurrence_path;
    let mut reply = result?;
    reply["made_by"] = json!("step");
    if let Some(why) = not_edited {
        reply["sketch_not_edited"] = json!(why);
    }
    Ok(reply)
}

/// Stretch by changing the sketch and Extrude that made `body` (`stretch_sketch`): the changes
/// are kept only when the part they rebuild has the box and volume the stretch gives;
/// otherwise the design goes back as it was and the answer says why.
fn edit_sketch(s: &mut Session, body: &str, stretches: &[AxisStretch]) -> std::result::Result<Value, String> {
    let state = s.model.state();
    let source = state.body(body).ok_or("the body is gone")?;
    // What the stretch makes, and where each cut is, as the Stretch step would work them out.
    let mut expected = source.body.clone();
    let mut planned = Vec::new();
    for one in stretches {
        let at = s.doc.eval(&one.at, Kind::Length).map_err(|e| e.to_string())?;
        let distance = s.doc.eval(&one.distance, Kind::Length).map_err(|e| e.to_string())?;
        let n = one.direction.vector();
        let base = match &one.from {
            Some(face) => face.0.dot(n),
            None => {
                let b = expected.exact_bounds().map_err(|e| e.to_string())?;
                b.min.dot(n).min(b.max.dot(n))
            }
        };
        expected = solidcraft_kernel::stretch(&expected, n * (base + at), n, distance).map_err(|e| e.to_string())?;
        planned.push(stretch_sketch::Planned { direction: one.direction, level: base + at, distance });
    }
    let (edited, changes) = stretch_sketch::changes(s, &state, body, &planned)?;
    let errors_before = s.model.results.iter().filter(|r| r.error.is_some()).count();
    let saved = s.doc.clone();
    let solved = s.solved_sketches();
    let undo = |s: &mut Session, why: String| {
        *s.doc_mut() = (*saved).clone();
        s.refresh();
        Err(why)
    };
    *s.doc_mut() = edited;
    if let Err(e) = s.doc_mut().change_params_atomic(&changes) {
        return undo(s, e.to_string());
    }
    if let Err(e) = s.settle_value_edit(&solved) {
        return undo(s, e.to_string());
    }
    let after = s.model.state();
    if s.model.results.iter().filter(|r| r.error.is_some()).count() > errors_before {
        return undo(s, "a feature fails after the sketch changes".into());
    }
    if !stretch_sketch::same_part(&after, body, &expected) {
        return undo(s, "the changed sketch does not give the stretched part (its other side moved too)".into());
    }
    let feature = state.body(body).map(|b| b.feature).unwrap_or(0);
    let name = s.doc.feature(feature).map(|f| f.name.clone()).unwrap_or_default();
    Ok(json!({
        "feature": feature,
        "name": name,
        "bodies": after.bodies.iter().map(|b| b.name.clone()).collect::<Vec<_>>(),
        "created_bodies": [],
        "changed_bodies": [body],
        "removed_bodies": [],
        "warning": null,
        "ms": null,
        "made_by": "sketch",
        "changed": changes.iter().map(|(n, e)| json!({"parameter": n, "expression": e})).collect::<Vec<_>>(),
    }))
}

#[cfg(test)]
#[path = "stretch_tests.rs"]
mod tests;
