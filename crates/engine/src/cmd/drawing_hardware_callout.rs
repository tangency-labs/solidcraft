//! `drawing.hardware_callout.add` (#20112): a callout of a press-in hardware hole on a view, a note whose
//! text reads the hardware record ("4× FLUSH NUT M5") and follows it when the model changes
//! ([`solidcraft_doc::drawing::hardware_callout`]). On a view of the model its leader ends on the centre
//! of the hole's circular edge, picked and saved by name as a dimension's centre is; on a flat pattern
//! view, which draws no model edge, it ends on the hole in the flat. What it says is read when it is
//! drawn or inspected, so the saved note keeps only the text it had when added, for a lost hole.
//! Adding is one undo step of the drawing; the note is edited and deleted as any note is.

use std::sync::Arc;

use serde_json::Value;
use solidcraft_doc::drawing::hardware_callout::{self, HardwareRef};
use solidcraft_doc::drawing::note::{Anchor, Note};
use solidcraft_geom::cell::{Content, HAlign, Part};
use solidcraft_geom::drawing_view::is_paper_coordinate;

use super::CommandSpec;
use super::drawing_note::answer;
use super::drawing_view::check_drawn;
use crate::params::bad;
use crate::schema::drawing_hardware_callout::AddParams;
use crate::schema::drawing_note::NoteResult;
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const ADD: &str = "drawing.hardware_callout.add";
const ADD_CONTRACT: TypedContract<AddParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(ADD, "Add Hardware Callout", crate::checked!("drawing.hardware_callout.add", add))
    .begins_edit()
    .icon("new")
    .in_drawing()
    .doc_only()
    .contract(&ADD_CONTRACT)
    .output(output_schema_for::<NoteResult>)
    .params(
        "view: a view of the part, or its flat pattern view; feature: the feature whose hole carries press-in hardware (the Hole feature, or the \
         pattern or mirror that copied it: sheet.flat_pattern lists each hardware hole's feature); copy?: which of its holes, from 0 (default 0); \
         at: [x, y] on paper in mm from the view's centre; corner?: {centre: {body, index}, path?} the centre of the hole's circular edge \
         as drawing.dimension.add picks a centre (required on a view of the model, where the leader ends there; refused on a flat pattern view, \
         where it ends on the hole in the flat) → what drawing.note.inspect answers for it. The note reads the hardware record: its text is \
         the count of that hardware on the sheet, its kind, thread and a stud's or standoff's length (\"4× FLUSH NUT M5\"), and follows the \
         record. A hole that is gone makes it lost, keeping its last text and drawing no leader; it never moves to another hole. Refused for \
         a feature with no hardware hole now or a view that doesn't show the part. One undo step; edit and delete it as a note.",
    )];

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let request: AddParams = crate::schema::read(ADD, p)?;
    let [x, y] = request.at;
    if !is_paper_coordinate(x) || !is_paper_coordinate(y) {
        return Err(bad(ADD, "`at` is a place on paper"));
    }
    let hole = HardwareRef { feature: request.feature.0, copy: request.copy };
    hole.validate().map_err(|e| bad(ADD, e.to_string()))?;
    let record = s.drawing(ADD)?.clone();
    let view = request.view.0;
    let resolved = record.resolve_view(view).map_err(|_| bad(ADD, format!("the drawing has no view {}", solidcraft_doc::ids::encode(view))))?;
    let state = s.model.state();
    let read = hardware_callout::read(&state.surface_sheets, &hole).map_err(|why| bad(ADD, why))?;
    let anchor = match (&resolved.flat, request.corner) {
        (Some(of), None) if of.body == read.body => Anchor::View(view),
        (Some(of), None) => {
            return Err(bad(ADD, format!("the view is the flat pattern of `{}`, not of `{}`, which has the hardware", of.body, read.body)));
        }
        (Some(_), Some(_)) => return Err(bad(ADD, "a flat pattern view's leader ends on the hole in the flat: give no `corner`")),
        (None, None) => return Err(bad(ADD, "`corner`: pick the centre of the hole's circular edge, where the leader ends")),
        (None, Some(corner)) => {
            let point = s.doc.corner_reference(&s.model, &corner.pick, &corner.path).map_err(|why| bad(ADD, why.to_string()))?;
            if !point.centre {
                return Err(bad(ADD, "`corner` is the centre of the hole's circular edge, not a corner"));
            }
            Anchor::Corner { view, point }
        }
    };
    // The text it has now, kept for when the hole is lost.
    let style = crate::drawing_hardware_callout::style(s, &record);
    let text = hardware_callout::text(&read, &style).map_err(|e| bad(ADD, e.to_string()))?.join("\n");
    let mut note = Note {
        id: 0,
        anchor,
        at: request.at,
        content: Content(vec![Part::Text(text)]),
        cap_mm: None,
        align: Some(HAlign::Left),
        branches: Vec::new(),
        hardware: Some(hole),
    };
    if let Some(view) = note.anchor.view() {
        check_drawn(&record, view, note.paths(), ADD)?;
    }
    note.id = record.allocate_id(&mut s.ids).map_err(|e| bad(ADD, e.to_string()))?;
    let id = note.id;
    s.edit_drawing(ADD, |d| {
        d.notes.add([Arc::new(note)]);
        Ok(())
    })?;
    answer(s, ADD, id)
}

#[cfg(test)]
#[path = "drawing_hardware_callout_tests.rs"]
mod tests;
