//! `drawing.copy` and `drawing.paste` (#13065): views, sheets and tables copied from one drawing
//! and pasted into the same one, another drawing of the design, or a drawing of another design.
//!
//! A copy is a snapshot of the drawing: every record is behind its own `Arc`, so it costs a few
//! vectors of pointers, carries the image files' bytes, and no later edit reaches it. A paste puts
//! the records in through the drawing record's one copier
//! ([`solidcraft_doc::drawing::Drawing::paste_records`]), which says what each kind keeps. Then,
//! with `to` or from another design, the pasted views are made to show that part as a replace does
//! ([`super::drawing_source::Retarget`]): each annotation that finds its one thing on it moves, and
//! the rest keep their old record and aren't drawn. One undo step; no model is evaluated.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::Value;
use solidcore_record::Vec2;
use solidcraft_doc::drawing::paste::Placement;
use solidcraft_doc::drawing::table::Place;
use solidcraft_doc::drawing::{Drawing, OnSheet};
use solidcraft_doc::ids;

use super::CommandSpec;
use super::drawing_source::{Follows, Origin, Retarget, Scope};
use crate::params::bad;
use crate::schema::drawing_clipboard::{CopyParams, CopyResult, LeftOutResult, PasteParams, PasteResult, PastedResult};
use crate::schema::drawing_source::ToParam;
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, ForeignCopy, Result, Session};

const COPY: &str = "drawing.copy";
const PASTE: &str = "drawing.paste";
const COPY_CONTRACT: TypedContract<CopyParams> = TypedContract::new();
const PASTE_CONTRACT: TypedContract<PasteParams> = TypedContract::new();
/// The most records of one kind a copy names.
const MAX_NAMED: usize = 1000;
/// Why a revision tag isn't pasted.
pub(super) const TAGS_STAY: &str = "a revision tag shows a row of its own drawing's revision record";

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new(COPY, "Copy Drawing Items", crate::checked!("drawing.copy", copy))
        .icon("copy")
        .in_drawing()
        .doc_only()
        .noundo()
        .contract(&COPY_CONTRACT)
        .output(output_schema_for::<CopyResult>)
        .params(
            "exactly one of views: [ids] (of one sheet; each with its dimensions, balloons and notes), sheets: [ids or names] \
             (each with everything on it but its revision tags), tables: [ids] → what was copied, by kind. A snapshot: later \
             edits don't change what is pasted. It is also what another design pastes with drawing.paste's from_design. No \
             undo step.",
        ),
    CommandSpec::new(PASTE, "Paste Drawing Items", crate::checked!("drawing.paste", paste))
        .icon("paste")
        .in_drawing()
        .doc_only()
        .contract(&PASTE_CONTRACT)
        .output(output_schema_for::<PasteResult>)
        .params(
            "sheet?: id or name, where copied views and tables go (default: the first sheet); position_mm?: [x, y] for the \
             centre of the first view drawing.copy listed, the others keeping their spacing (default: each keeps its place); \
             every view centre and \
             table place must be on the paper. Copied sheets come whole, as new sheets, keeping their names when free (else \
             the next Sheet N), so they take neither. to?: {component, path?}, as drawing.source.replace's: the part the \
             views show (default: the parts they showed; from another design, this design itself); from_design?: true to \
             paste what another design copied → {pasted: [{from, id, kind}], kept, lost, dropped_hide, left_out, warnings}. \
             A projected, pictorial or auxiliary view whose parent isn't pasted, or every one when the drawings' projection \
             angles differ, becomes a base view that looks as it did. A section cuts its parent's copy, or its parent in the \
             drawing it came from; else the paste is refused. A view takes a letter only as any view does, never one another \
             view holds. A sheet's sketches and fills come with it. With to, or from another design, the views must show one part, and an \
             annotation moves when it finds its one thing on the new part (else it keeps its old record and isn't drawn); \
             the sheets' documented component and parts lists become to. Revision tags aren't copied. One undo step; no \
             model is evaluated.",
        ),
];

/// What `drawing.copy` copied: the drawing as it was then, and which of its records.
#[derive(Clone, Debug)]
pub struct DrawingClip {
    pub drawing: Arc<Drawing>,
    /// The sheets copied whole, in the drawing's order; what is on them is in `objects`.
    pub sheets: Vec<u64>,
    pub objects: OnSheet,
}

fn copy(s: &mut Session, p: &Value) -> Result<Value> {
    let request: CopyParams = crate::schema::read(COPY, p)?;
    let d = s.drawing(COPY)?.clone();
    let named = |count: usize, what: &str| {
        if (1..=MAX_NAMED).contains(&count) { Ok(()) } else { Err(bad(COPY, format!("`{what}` lists 1 to {MAX_NAMED} {what}"))) }
    };
    let (sheets, objects) = match (request.views, request.sheets, request.tables) {
        (Some(views), None, None) => {
            named(views.len(), "views")?;
            let views: BTreeSet<u64> = views.iter().map(|v| v.0).collect();
            let mut on = BTreeSet::new();
            for id in &views {
                on.insert(d.view(*id).ok_or_else(|| bad(COPY, format!("the drawing has no view {}", ids::encode(*id))))?.sheet);
            }
            if on.len() > 1 {
                return Err(bad(COPY, "the views are on more than one sheet: copy views of one sheet at a time"));
            }
            (Vec::new(), d.with_contents(views.into_iter().collect()))
        }
        (None, Some(names), None) => {
            named(names.len(), "sheets")?;
            let at = names.iter().map(|name| d.sheet_index(name).map_err(|e| bad(COPY, e.to_string()))).collect::<Result<BTreeSet<usize>>>()?;
            let sheets: Vec<u64> = at.iter().filter_map(|i| d.sheets.get(*i)).map(|sheet| sheet.id).collect();
            (sheets.clone(), joined(sheets.iter().map(|id| d.on_sheet(*id))))
        }
        (None, None, Some(tables)) => {
            named(tables.len(), "tables")?;
            let tables: BTreeSet<u64> = tables.iter().map(|t| t.0).collect();
            if let Some(id) = tables.iter().find(|id| d.tables.by_id(**id).is_none()) {
                return Err(bad(COPY, format!("the drawing has no table {}", ids::encode(*id))));
            }
            (Vec::new(), OnSheet { tables: tables.into_iter().collect(), ..OnSheet::default() })
        }
        _ => return Err(bad(COPY, "give exactly one of views, sheets and tables")),
    };
    let encoded = |list: &[u64]| list.iter().map(|id| ids::encode(*id)).collect::<Vec<_>>();
    let reply = CopyResult {
        sheets: encoded(&sheets),
        views: encoded(&objects.views),
        dimensions: encoded(&objects.dimensions),
        balloons: encoded(&objects.balloons),
        notes: encoded(&objects.notes),
        images: encoded(&objects.images),
        tables: encoded(&objects.tables),
        param_warnings: Vec::new(),
    };
    let clip = Arc::new(DrawingClip { drawing: d, sheets, objects });
    s.drawing_clipboard = Some(clip.clone());
    s.foreign_copy = Some(Arc::new(ForeignCopy::of_drawing(s, clip)));
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// What is on several sheets, each list by id. Every list is named, with no `..`: a new kind of
/// record on a sheet fails to build here, and in [`Drawing::paste_records`], until it says how it
/// is copied.
fn joined(sheets: impl Iterator<Item = OnSheet>) -> OnSheet {
    let mut all = OnSheet::default();
    for OnSheet { views, dimensions, balloons, images, tags, tables, notes, sketches, fills } in sheets {
        all.views.extend(views);
        all.dimensions.extend(dimensions);
        all.balloons.extend(balloons);
        all.images.extend(images);
        all.tags.extend(tags);
        all.tables.extend(tables);
        all.notes.extend(notes);
        all.sketches.extend(sketches);
        all.fills.extend(fills);
    }
    let OnSheet { views, dimensions, balloons, images, tags, tables, notes, sketches, fills } = &mut all;
    for list in [views, dimensions, balloons, images, tags, tables, notes, sketches, fills] {
        list.sort_unstable();
    }
    all
}

/// The warning a paste across projection angles gives.
pub(super) fn angles_differ(views: usize) -> String {
    let noun = if views == 1 { "view is" } else { "views are" };
    format!(
        "the copy is from a drawing of the other projection angle: {views} projected, pictorial or auxiliary {noun} pasted as base views that look as they did"
    )
}

fn paste(s: &mut Session, p: &Value) -> Result<Value> {
    let request: PasteParams = crate::schema::read(PASTE, p)?;
    // The copy, and the design it was made in when that is another one.
    let foreign = match request.from_design {
        Some(true) => Some(s.foreign_copy.clone().ok_or_else(|| bad(PASTE, "nothing was copied in another design"))?),
        _ => None,
    };
    let clip = match &foreign {
        Some(copy) => copy.drawing.clone(),
        None => s.drawing_clipboard.clone(),
    };
    let clip = clip.ok_or_else(|| bad(PASTE, "nothing copied (drawing.copy first)"))?;
    let (source, on) = (&*clip.drawing, &clip.objects);
    let mut d = Drawing::clone(s.drawing(PASTE)?);
    let ours = d.design_source().ok_or_else(|| bad(PASTE, "the drawing draws no design"))?;
    let whole = !clip.sheets.is_empty();
    if whole && (request.sheet.is_some() || request.position_mm.is_some()) {
        return Err(bad(PASTE, "copied sheets are pasted whole, as new sheets: leave out sheet and position_mm"));
    }
    let onto = match &request.sheet {
        Some(named) => d.sheets.get(d.sheet_index(named).map_err(|e| bad(PASTE, e.to_string()))?),
        None => d.sheets.first(),
    };
    let onto = onto.map(|sheet| sheet.id).ok_or_else(|| bad(PASTE, "the drawing has no sheet"))?;
    let err = |e: solidcraft_doc::DocError| bad(PASTE, e.to_string());
    let first = on.views.first().map(|id| source.view_centre(*id)).transpose().map_err(err)?;
    let by = match (first, request.position_mm) {
        (Some(from), Some(to)) => to.0 - from,
        _ => Vec2::ZERO,
    };
    let made = d.paste_records(source, on, &clip.sheets, Placement { onto, by, source: ours }, &mut s.ids).map_err(err)?;
    if !whole {
        on_the_paper(&d, onto, &made.views, &made.tables)?;
    }

    let mut reply = PasteResult {
        pasted: made.ids.iter().map(|(kind, from, id)| PastedResult { from: ids::encode(*from), id: ids::encode(*id), kind }).collect(),
        kept: Vec::new(),
        lost: Vec::new(),
        dropped_hide: Vec::new(),
        left_out: on.tags.iter().map(|id| LeftOutResult { id: ids::encode(*id), kind: "revision tag", why: TAGS_STAY }).collect(),
        warnings: (made.based_across > 0).then(|| angles_differ(made.based_across)).into_iter().collect(),
        param_warnings: Vec::new(),
    };
    // With `to`, or from another design (this design itself, by default), the views show that part.
    let itself = ToParam { component: 0, path: None };
    let to = match (&request.to, &foreign) {
        (Some(to), _) => Some(to),
        (None, Some(_)) => Some(&itself),
        (None, None) => None,
    };
    if let Some(to) = to {
        let (from_doc, from_state) = match &foreign {
            Some(copy) => (&*copy.doc, copy.model.state()),
            None => (&*s.doc, s.model.state()),
        };
        let origin = Origin { doc: from_doc, state: &from_state, drawing: source, bases: &made.old_bases, other_design: foreign.is_some() };
        let retarget = Retarget::new(s, origin, to, PASTE)?;
        let set = |list: &[u64]| list.iter().copied().collect::<BTreeSet<u64>>();
        let (views, sheets, tables) = (set(&made.views), set(&made.sheets), set(&made.tables));
        // Every pasted sheet that documents a component, and every pasted parts list, documents and
        // lists the part, as before Retarget was shared with the replace.
        let follows = Follows::Any;
        let within = Scope { views: &views, bases: &made.bases, sheets: &sheets, tables: &tables, source: ours, follows };
        let done = retarget.apply(s, &mut d, within);
        (reply.kept, reply.lost, reply.dropped_hide) = (done.kept, done.lost, done.dropped_hide);
    }
    s.edit_drawing(PASTE, |live| {
        *live = d;
        Ok(())
    })?;
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// Refused when one of the pasted `views`' centre, or one of the `tables` placed at a point, lies
/// off the paper of sheet `onto`, where they went.
fn on_the_paper(d: &Drawing, onto: u64, views: &[u64], tables: &[u64]) -> Result<()> {
    let sheet = d.sheets.iter().find(|sheet| sheet.id == onto).ok_or_else(|| bad(PASTE, "the drawing has no such sheet"))?;
    let size = sheet.sheet.size_mm().map_err(|e| bad(PASTE, e.to_string()))?;
    let off = |at: Vec2| !((0.0..=size.x).contains(&at.x) && (0.0..=size.y).contains(&at.y));
    let refuse = |what: &str, id: u64| bad(PASTE, format!("{what} {} would sit off the paper of sheet {}", ids::encode(id), sheet.name));
    for id in views {
        if off(d.view_centre(*id).map_err(|e| bad(PASTE, e.to_string()))?) {
            return Err(refuse("view", *id));
        }
    }
    for table in tables.iter().filter_map(|id| d.tables.by_id(*id)) {
        if let Place::At([x, y]) = table.place
            && off(Vec2::new(x, y))
        {
            return Err(refuse("table", table.id));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "drawing_clipboard_tests.rs"]
mod tests;
