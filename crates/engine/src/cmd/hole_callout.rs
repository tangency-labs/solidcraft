//! Hole callouts (W29a, #7605): what a drawing's callout of one hole says, worked out from the
//! model's Hole feature ([`solidcraft_doc::drawing::hole_callout`]). Read-only: nothing is saved
//! and nothing is placed on a sheet. It works with a drawing open or with none; with one open,
//! the drawing's fixed unit and its content language are the defaults.

use serde_json::Value;
use solidcraft_doc::drawing::Units;
use solidcraft_doc::drawing::hole_callout::{self, DEFAULT_DECIMALS, HoleRef, Resolution, TextStyle};
use solidcraft_doc::expr;

use super::CommandSpec;
use crate::params::bad;
use crate::schema::hole_callout::{CalloutParams, CalloutResult, CalloutValues, HoleResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const CMD: &str = "drawing.hole_callout";
const CONTRACT: TypedContract<CalloutParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(CMD, "Hole Callout", crate::checked!("drawing.hole_callout", callout))
    .icon("inspect")
    .any_part()
    .noundo()
    .contract(&CONTRACT)
    .output(output_schema_for::<CalloutResult>)
    .params(
        "feature: the Hole feature's id; point?: its sketch point id (one of the holes it drills at sketch points; none for a hole placed by \
         position); units?: a length unit (mm, in, yd, …; default the open drawing's fixed unit, else the design's); decimals?: 0…6 (default 2; \
         inches show at least 3); language?: the content language, a BCP 47 tag (default the open drawing's, else en) → hole: {feature, \
         point?}; status: resolved | suppressed (the feature is) | failed (it did not evaluate; a later feature removed the hole, moved it \
         some way other than solid.move, or changed its diameter or depth; or it is in sheet metal, not supported yet) | lost (the feature or \
         the point is gone; never moved to another hole); reason? (failed, lost); callout? {count (holes the feature drills), diameter (the \
         drilled one), thread?, thread_depth? (from the face the hole starts on, past any counterbore or countersink), extent (this hole's): {type: through} | {type: blind, depth (from the face the \
         hole starts on)}, form: {type: simple} | {type: drilled, tip_angle} | {type: counterbore, diameter, depth} | {type: countersink, \
         diameter, angle}, start, start_face, direction (in the model as it is now, after a solid.move of the body, in the hole's own \
         component)} in mm and radians; lines?, text? (the lines joined by newlines). Works with a drawing open or with none. Holes of other \
         features are not supported yet",
    )];

fn callout(s: &mut Session, p: &Value) -> Result<Value> {
    let request: CalloutParams = crate::schema::read(CMD, p)?;
    // With a drawing open, its fixed unit and its language; else the design's unit and English.
    let drawing = s.open_drawing.and_then(|id| s.doc.drawing(id));
    let units = match (request.units, drawing.map(|d| &d.units)) {
        (Some(u), _) => u,
        (None, Some(Units::Fixed { unit })) => unit.clone(),
        (None, _) => expr::length_unit_or_mm(&s.doc.units).to_string(),
    };
    let language = request.language.or_else(|| drawing.map(|d| d.language.clone())).unwrap_or_else(|| "en".to_string());
    let style = TextStyle { units, decimals: request.decimals.map_or(DEFAULT_DECIMALS, |d| d.0), language };
    style.validate().map_err(|e| bad(CMD, e.to_string()))?;
    let hole = HoleRef { feature: request.feature.0, point: request.point };
    let resolution = hole_callout::resolve(&s.doc, &s.model, &hole).map_err(|e| bad(CMD, e.to_string()))?;
    let mut reply = CalloutResult {
        hole: HoleResult { feature: hole.feature, point: hole.point },
        status: String::new(),
        reason: None,
        callout: None,
        lines: None,
        text: None,
        param_warnings: Vec::new(),
    };
    match &resolution {
        Resolution::Resolved { callout } => {
            let lines = hole_callout::text(callout, &style).map_err(|e| bad(CMD, e.to_string()))?;
            reply.status = "resolved".into();
            reply.callout = Some(CalloutValues::from(callout));
            reply.text = Some(lines.join("\n"));
            reply.lines = Some(lines);
        }
        Resolution::Suppressed => reply.status = "suppressed".into(),
        Resolution::Failed { reason } => {
            reply.status = "failed".into();
            reply.reason = Some(reason.clone());
        }
        Resolution::Lost { reason } => {
            reply.status = "lost".into();
            reply.reason = Some(reason.clone());
        }
    }
    serde_json::to_value(&reply).map_err(|e| bad(CMD, e.to_string()))
}

#[cfg(test)]
#[path = "hole_callout_tests.rs"]
mod tests;
