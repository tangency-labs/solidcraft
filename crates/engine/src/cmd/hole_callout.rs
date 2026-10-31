//! Hole callouts (W29a, #7605): what a drawing's callout of one hole says, worked out from the
//! model's Hole feature ([`solidcraft_doc::drawing::hole_callout`]). Read-only: nothing is saved
//! and nothing is placed on a sheet.

use serde_json::{Value, json};
use solidcraft_doc::drawing::hole_callout::{self, DEFAULT_DECIMALS, HoleRef, Resolution, TextStyle};

use super::CommandSpec;
use crate::params::{bad, str_};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("drawing.hole_callout", "Hole Callout", callout).icon("inspect").noundo().params(
    "feature: the Hole feature's id; point?: its sketch point id (one of the holes it drills at sketch points; none for a hole placed by \
     position); units?: mm|cm|m|in|ft (default the design's); decimals?: 0…6 (default 2; inches show at least 3); language?: the drawing's content language, a BCP 47 \
     tag (default en) → hole: {feature, point?}; status: resolved | suppressed (the feature is) | failed (it did not evaluate, or a later feature removed the hole) | lost (the \
     feature or the point is gone; never moved to another hole); reason? (failed, lost); callout? {count (holes the feature drills), diameter \
     (the drilled one), thread?, extent: {type: through} | {type: blind, depth (from the face the hole starts on)}, form: {type: simple} | \
     {type: drilled, tip_angle} | {type: counterbore, diameter, depth} | {type: countersink, diameter, angle}, start, start_face, direction} \
     in mm and radians; lines?, text? (the lines joined by newlines). Holes of other features are not supported yet",
)];

fn callout(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.hole_callout";
    let feature = p.get("feature").and_then(Value::as_u64).filter(|id| *id >= 1).ok_or_else(|| bad(cmd, "`feature` must be a Hole feature's id"))?;
    let point = match p.get("point") {
        None | Some(Value::Null) => None,
        Some(Value::String(id)) => Some(id.clone()),
        Some(_) => return Err(bad(cmd, "`point` must be a sketch point id")),
    };
    let decimals = match p.get("decimals") {
        None | Some(Value::Null) => DEFAULT_DECIMALS,
        Some(v) => v.as_u64().and_then(|d| u8::try_from(d).ok()).ok_or_else(|| bad(cmd, "`decimals` must be 0 to 6"))?,
    };
    let units = match str_(p, "units") {
        Some(u) => u.to_string(),
        // The design's own unit, as a drawing that follows the model shows it.
        None => match s.doc.units.trim() {
            u @ ("mm" | "cm" | "m" | "in" | "ft") => u.to_string(),
            _ => "mm".to_string(),
        },
    };
    let style = TextStyle { units, decimals, language: str_(p, "language").unwrap_or("en").to_string() };
    style.validate().map_err(|e| bad(cmd, e.to_string()))?;
    let hole = HoleRef { feature, point };
    let resolution = hole_callout::resolve(&s.doc, &s.model, &hole).map_err(|e| bad(cmd, e.to_string()))?;
    let mut out = serde_json::to_value(&resolution).map_err(|e| bad(cmd, e.to_string()))?;
    if let Some(o) = out.as_object_mut() {
        o.insert("hole".into(), json!(hole));
        if let Resolution::Resolved { callout } = &resolution {
            let lines = hole_callout::text(callout, &style).map_err(|e| bad(cmd, e.to_string()))?;
            o.insert("text".into(), json!(lines.join("\n")));
            o.insert("lines".into(), json!(lines));
        }
    }
    Ok(out)
}

#[cfg(test)]
#[path = "hole_callout_tests.rs"]
mod tests;
