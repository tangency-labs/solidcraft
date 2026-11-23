//! Chamfer notes (#13028): what a drawing's note of one chamfer says, worked out from the model's
//! Chamfer feature ([`solidcraft_doc::drawing::chamfer_note`]). Read-only, as `drawing.hole_callout`
//! is: nothing is saved and nothing is placed on a sheet (a note with a leader places the text). It
//! works with a drawing open or with none; with one open, the drawing's fixed unit and its content
//! language are the defaults.

use serde_json::Value;
use solidcraft_doc::drawing::chamfer_note::{self, ChamferRef, Resolution};

use super::CommandSpec;
use crate::params::bad;
use crate::schema::chamfer_note::{ChamferResult, NoteParams, NoteResult, NoteValues};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const CMD: &str = "drawing.chamfer_note";
const CONTRACT: TypedContract<NoteParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(CMD, "Chamfer Note", crate::checked!("drawing.chamfer_note", note))
    .icon("inspect")
    .any_part()
    .noundo()
    .contract(&CONTRACT)
    .output(output_schema_for::<NoteResult>)
    .params(
        "feature: the Chamfer feature's id; edge: the persistent name of an edge it cuts (as model.edges names it); count?: true (n × \
         (…) in front when the feature cuts more than one edge); units?: a length unit (mm, in, yd, …; default the open drawing's fixed \
         unit, else the design's); decimals?: 0…6 (default 2; inches show at least 3); language?: a BCP 47 tag (default the open \
         drawing's, else en) → chamfer: {feature, edge}; status: resolved | suppressed (the feature is) | failed (it did not evaluate, \
         or a later feature removed the chamfer) | lost (the feature is gone; never moved to another chamfer); reason? (failed, lost); \
         note? {count (edges the feature cuts), size: {type: equal, distance} | {type: angle, distance, angle} | {type: distances, \
         up (along the face facing up most), other}} in mm and radians; text? (d × 45°, d × a, up × other). An edge the feature \
         doesn't cut is refused. Works with a drawing open or with none. Chamfers of other features are not supported yet",
    )];

fn note(s: &mut Session, p: &Value) -> Result<Value> {
    let request: NoteParams = crate::schema::read(CMD, p)?;
    let style = super::hole_callout::text_style(s, request.units, request.decimals, request.language, CMD)?;
    let chamfer = ChamferRef { feature: request.feature.0, edge: request.edge };
    let resolution = chamfer_note::resolve(&s.doc, &s.model, &chamfer).map_err(|e| bad(CMD, e.to_string()))?;
    let mut reply = NoteResult {
        chamfer: ChamferResult { feature: chamfer.feature, edge: chamfer.edge },
        status: String::new(),
        reason: None,
        note: None,
        text: None,
        param_warnings: Vec::new(),
    };
    match &resolution {
        Resolution::Resolved { note } => {
            reply.text = Some(chamfer_note::text(note, &style, request.count).map_err(|e| bad(CMD, e.to_string()))?);
            reply.status = "resolved".into();
            reply.note = Some(NoteValues::from(note));
        }
        Resolution::Suppressed => reply.status = "suppressed".into(),
        Resolution::Failed { reason } => (reply.status, reply.reason) = ("failed".into(), Some(reason.clone())),
        Resolution::Lost { reason } => (reply.status, reply.reason) = ("lost".into(), Some(reason.clone())),
    }
    serde_json::to_value(&reply).map_err(|e| bad(CMD, e.to_string()))
}

#[cfg(test)]
#[path = "chamfer_note_tests.rs"]
mod tests;
