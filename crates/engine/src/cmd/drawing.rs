//! Drawing commands (#3890, K4): make, list, open, close and delete a design's drawings, and read
//! and change the open drawing's own settings. A drawing lives in its design's file
//! ([`solidcraft_doc::Document::drawings`]); while one is open, the drawing commands change it and
//! the model's wait ([`super::Part`]). Each change of a drawing is one undo step of that drawing,
//! and evaluates nothing; making and deleting one are steps of the model, whose list of drawings
//! they change.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_doc::bom::{self, Tree};
use solidcraft_doc::drawing::balloon::balloons_by_row;
use solidcraft_doc::drawing::table::Kind;
use solidcraft_doc::drawing::view::{MAX_VIEW_DEPTH, View, ViewKind};
use solidcraft_doc::drawing::{self, Drawing, Source, template};
use solidcraft_doc::drawing_view::ViewDirection;
use solidcraft_doc::{Document, ids};
use solidcraft_geom::cell::Warning;
use solidcraft_geom::drawing_view::ProjectedSide;
use solidcraft_geom::paper::line::LineGroup;

use super::CommandSpec;
use crate::params::{bad, str_};
use crate::schema::drawing::{
    DrawingCreateParams, DrawingSetParams, SheetTemplateChange, TemplateApplyParams, TemplateApplyResult, TemplateEntry, TemplateListParams,
    TemplateListResult, TemplateSaveParams, TemplateSaveResult,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::title_block_text::{self, CellText, Note};
use crate::{EngineError, Result, Session};

/// The longest template file path taken, in bytes, as the sheet export's.
const MAX_PATH_BYTES: usize = 4096;
const CREATE_CONTRACT: TypedContract<DrawingCreateParams> = TypedContract::new();
const SET_CONTRACT: TypedContract<DrawingSetParams> = TypedContract::new();
const TEMPLATE_APPLY_CONTRACT: TypedContract<TemplateApplyParams> = TypedContract::new();
const TEMPLATE_LIST_CONTRACT: TypedContract<TemplateListParams> = TypedContract::new();
const TEMPLATE_SAVE_CONTRACT: TypedContract<TemplateSaveParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("drawing.create", "Create Drawing", crate::checked!("drawing.create", create)).any_part().icon("new").doc_only().contract(&CREATE_CONTRACT).params(
        "name?: the drawing's name (default: the design's); template?: a built-in's name (drawing.template.list; default iso-a3-part for a part, iso-a2-assembly for an assembly, a design that places components) or a .json template file (drawing.template.save); language?: the drawing's content language, a BCP 47 tag (default: the template's, en for the built-ins); units?: \"follow_model\" or one length unit such as \"mm\" (default: the template's); projection?: first or third (angle) (default: the template's, third for the built-ins); font?: the font family its text is set in, by name (default: the template's, the built-in font for the built-ins); line_group?: the ISO 128 line group its lines are drawn in, by its wide line, one of 0.25, 0.35, 0.5, 0.7, 1, 1.4 or 2 (default: the template's, 0.5 for the built-ins). Adds a drawing of this design made from the template, with ids of its own, and opens it (one undo step of the model) → as drawing.inspect",
    ),
    CommandSpec::new("drawing.template.apply", "Apply Drawing Template", crate::checked!("drawing.template.apply", template_apply))
        .in_drawing()
        .icon("settings")
        .doc_only()
        .contract(&TEMPLATE_APPLY_CONTRACT)
        .output(output_schema_for::<TemplateApplyResult>)
        .params(
            "template: a built-in's name (drawing.template.list) or a .json template file (drawing.template.save); dry_run?: true to answer without changing anything. Each sheet takes the paper, drawing border and title block of the template's sheet in its place (its last for sheets past its own); text typed over a cell stays while the new block has a text cell of that name. Nothing moves → changed: [{sheet, paper, border, title_block, lost: [cells whose typed text is dropped]}] for each sheet that changes, outside: the views on a restyled sheet that fall outside its drawing space or over its title block (as drawing.sheet.set lists them), dry_run. One undo step, none when nothing changes.",
        ),
    CommandSpec::new("drawing.template.save", "Save as Drawing Template", crate::checked!("drawing.template.save", template_save))
        .in_drawing()
        .icon("save")
        .noundo()
        .doc_only()
        .contract(&TEMPLATE_SAVE_CONTRACT)
        .output(output_schema_for::<TemplateSaveResult>)
        .params(
            "path: a .json file → {path, bytes}. Writes the drawing open as a template: its sheets, borders, title blocks and the text typed over their cells, its projection, units, language and properties but its number, and nothing it draws or documents (its sources, views, dimensions, balloons, revision rows, and the component each sheet documents). drawing.create and drawing.template.apply take the file as their template; a drawing made from it keeps its own copy, so a later save changes no drawing.",
        ),
    CommandSpec::new("drawing.template.list", "List Drawing Templates", crate::checked!("drawing.template.list", template_list))
        .any_part()
        .icon("list")
        .noundo()
        .doc_only()
        .contract(&TEMPLATE_LIST_CONTRACT)
        .output(output_schema_for::<TemplateListResult>)
        .params(
            "→ templates: [{name, title, assembly: whether it is an assembly's, default: whether drawing.create starts from it for this design}], the built-ins: ISO A4 to A0 for a part and for an assembly, each with the ISO border, the built-in title block and its kind's general tolerances",
        ),
    CommandSpec::new("drawing.list", "List Drawings", list)
        .any_part()
        .icon("list")
        .noundo()
        .doc_only()
        .params("→ drawings: [{id, name, sheets (how many), open}], by name, then id"),
    CommandSpec::new("drawing.open", "Open Drawing", open)
        .any_part()
        .icon("open")
        .noundo()
        .doc_only()
        .params("drawing: id (from drawing.list) → as drawing.inspect. The drawing commands then change it; the model's wait until drawing.close"),
    CommandSpec::new("drawing.close", "Close Drawing", close)
        .any_part()
        .icon("close")
        .noundo()
        .doc_only()
        .params("→ closed: the drawing's id, or null when none was open"),
    CommandSpec::new("drawing.delete", "Delete Drawing", delete).any_part().icon("delete").doc_only().params(
        "drawing: id → deleted: its id; closed when it was open (one undo step of the model)",
    ),
    CommandSpec::new("drawing.inspect", "Inspect Drawing", inspect).in_drawing().icon("inspect").noundo().doc_only().params(
        "→ {id, name, units, projection, updates: automatic or deferred, language, font, line_group, font_note: why its text is set in the built-in font instead (the font isn't on this computer, or can't be set on paper), or null, properties, sources: [{id}], sheets: [{id, name, size_mm, orientation, title_block (its definition, or null), title_block_cells: [{name, text, value?, notes?}] (each text cell as the sheet shows it: its fields' values, nothing for one with none, or the text typed over it with the value it hides as `value`; `notes` are what doesn't fit on paper, each {fit: chars (with count and max), height or width}, shown whole, or {refused: why} for a label or value the drawing's font can't set, left off the sheet), title_block_overrides, dpi, line_group (its own, or null for the drawing's), documents, excluded, views: [{id, kind: base|projected, parent?, side?, label, dimensions: [ids], balloons: [ids]}], base views by id, each followed by those projected from it}], revisions: {rows: [{row, revision, description, designer, checker, date, hidden}], the oldest first (the newest is the drawing's revision), next: the revision a new row gets}}. Read-only.",
    ),
    CommandSpec::new("drawing.set", "Drawing Settings", crate::checked!("drawing.set", set))
        .in_drawing()
        .icon("settings")
        .doc_only()
        .contract(&SET_CONTRACT)
        .params("name?, language?: a BCP 47 tag, units?: \"follow_model\" or one length unit such as \"mm\", projection?: first or third (angle), which every projected view follows, updates?: automatic, or deferred: its views follow a changed model only on drawing.update, font?: the font family its text is set in, by name, never a file (\"\": the built-in font). line_group?: the ISO 128 line group its lines are drawn in, by its wide line: 0.25, 0.35, 0.5, 0.7, 1, 1.4 or 2; a sheet or a view may choose another. revision_numbering?: {start: 0 or 1}, the number new revisions count from: rows already there stay as typed. One undo step."),
    CommandSpec::new("drawing.properties", "Drawing Properties", properties).in_drawing().icon("settings").noundo().doc_only().params(
        "→ metadata: the drawing's properties as saved (other tools' keys too); properties: project, part_number, revision, author, organization, description and license, each its text or null. The design properties' rules (document.metadata).",
    ),
    CommandSpec::new("drawing.properties.set", "Change Drawing Properties", properties_set).in_drawing().icon("settings").doc_only().params(
        "key and value, or values: {key: value, …} (all or nothing, one undo step), as document.metadata.set takes them → metadata, changed: [keys]",
    ),
    CommandSpec::new("drawing.properties.remove", "Remove Drawing Property", properties_remove)
        .in_drawing()
        .icon("delete")
        .doc_only()
        .params("key, or keys: [keys] → metadata, removed: [keys that were there]"),
];

/// A `line_group` as given, for a drawing, a sheet or a view: one of ISO 128's groups, by its wide line.
pub(crate) fn line_group_of(v: &Value) -> std::result::Result<LineGroup, String> {
    serde_json::from_value(v.clone()).map_err(|_| format!("`line_group` is one of {}", LineGroup::NAMES.join(", ")))
}

/// [`line_group_of`], refused as `cmd`.
pub(super) fn line_group(cmd: &str, v: &Value) -> Result<LineGroup> {
    line_group_of(v).map_err(|e| bad(cmd, e))
}

/// `font` as given: a font family's name, never a file (drawings name fonts by family, so they
/// travel).
fn font<'a>(cmd: &str, font: Option<&'a str>) -> Result<Option<&'a str>> {
    if let Some(family) = font {
        solidcraft_sketch::check_family(family).map_err(|e| bad(cmd, e.to_string()))?;
    }
    Ok(font.map(str::trim))
}

/// An optional text parameter: absent, or a string.
fn text<'a>(cmd: &str, p: &'a Value, key: &str) -> Result<Option<&'a str>> {
    match p.get(key) {
        None => Ok(None),
        Some(_) => str_(p, key).map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be text"))),
    }
}

/// The design's drawing `p.drawing` names, by id.
fn named(cmd: &str, s: &Session, p: &Value) -> Result<u64> {
    let id = str_(p, "drawing").and_then(ids::decode).ok_or_else(|| bad(cmd, "`drawing` must be a drawing's id (drawing.list)"))?;
    s.doc.drawing(id).map(|d| d.id).ok_or_else(|| bad(cmd, format!("the design has no drawing {}", ids::encode(id))))
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.create";
    if s.doc.features.is_empty() {
        return Err(bad(cmd, "the design is empty: there is nothing to draw yet"));
    }
    drawing::check_count(s.doc.drawings.len() + 1).map_err(|e| bad(cmd, e.to_string()))?;
    let request: DrawingCreateParams = crate::schema::read(cmd, p)?;
    let name = request.name.unwrap_or_else(|| s.doc.name.clone());
    let fail = |e: solidcraft_doc::DocError| bad(cmd, e.to_string());
    let from = request.template.as_deref().unwrap_or_else(|| template::default_for(template::is_assembly(&s.doc)));
    let chosen = template_named(cmd, from)?;
    // Not the id of a drawing undo or redo could bring back either.
    let steps = s.undo.iter().chain(&s.redo).filter_map(|step| step.part);
    let taken: BTreeSet<u64> = s.doc.drawings.iter().map(|d| d.id).chain(steps).collect();
    let mut d = template::instantiate(&chosen, &name, &taken, &mut s.ids).map_err(fail)?;
    if let Some(language) = request.language {
        d.language = language;
    }
    if let Some(projection) = request.projection {
        d.projection = projection;
    }
    if let Some(group) = request.line_group {
        d.line_group = group;
    }
    let source = d.allocate_id(&mut s.ids).map_err(fail)?;
    d.sources.push(Source { id: source });
    if let Some(units) = request.units {
        d.units = units;
    }
    if let Some(font) = font(cmd, request.font.as_deref())? {
        d.font = font.to_string();
    }
    d.validate().map_err(fail)?;
    s.open_drawing = Some(d.id);
    s.doc_mut().put_drawing(Arc::new(d));
    inspect(s, &Value::Null)
}

fn template_apply(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.template.apply";
    let fail = |e: solidcraft_doc::DocError| bad(cmd, e.to_string());
    let from = text(cmd, p, "template")?.ok_or_else(|| bad(cmd, "`template` must be a template's name (drawing.template.list)"))?;
    let chosen = template_named(cmd, from)?;
    let dry_run = match p.get("dry_run") {
        None => false,
        Some(Value::Bool(dry_run)) => *dry_run,
        Some(_) => return Err(bad(cmd, "`dry_run` is true or false")),
    };
    let (applied, changed) = template::apply(s.drawing(cmd)?, &chosen).map_err(fail)?;
    // The views on a sheet whose room changed: listed when they fall outside or over the block, never moved.
    let views: Vec<u64> = changed.iter().filter(|c| c.paper || c.border || c.title_block).flat_map(|c| applied.on_sheet(c.sheet).views).collect();
    let outside = super::drawing_view::outside_results(crate::drawing_view::outside(s, &applied, &views, cmd)?);
    let listed = changed
        .iter()
        .map(|c| SheetTemplateChange {
            sheet: ids::encode(c.sheet),
            paper: c.paper,
            border: c.border,
            title_block: c.title_block,
            lost: c.lost.clone(),
        })
        .collect();
    if !dry_run && !changed.is_empty() {
        s.edit_drawing(cmd, |d| {
            *d = applied;
            Ok(())
        })?;
    }
    let reply = TemplateApplyResult { changed: listed, outside, dry_run, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// The template `from` names: a built-in (drawing.template.list), or a template file read through
/// the file check as every file is.
fn template_named(cmd: &str, from: &str) -> Result<Drawing> {
    if let Some(made) = template::built_in(from) {
        return made.map_err(|e| bad(cmd, e.to_string()));
    }
    if !template_path(from) {
        return Err(bad(cmd, format!("no template `{from}`: a built-in's name (drawing.template.list) or a .json template file")));
    }
    let text = solidcraft_io::vfs::read_to_string(from, drawing::MAX_DRAWING_BYTES).map_err(|e| bad(cmd, format!("template `{from}`: {e}")))?;
    template::read(&text).map_err(|e| bad(cmd, format!("template `{from}`: {e}")))
}

/// Whether `path` can be a template file's: a .json file, at most [`MAX_PATH_BYTES`] long.
fn template_path(path: &str) -> bool {
    path.len() <= MAX_PATH_BYTES && Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("json"))
}

fn template_save(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.template.save";
    let path = text(cmd, p, "path")?.filter(|path| template_path(path)).ok_or_else(|| bad(cmd, "`path` must be a .json file"))?;
    solidcraft_io::vfs::check(path).map_err(|e| EngineError::Other(e.to_string()))?;
    let made = template::from_drawing(s.drawing(cmd)?).map_err(|e| bad(cmd, e.to_string()))?;
    let text = made.to_json().map_err(|e| bad(cmd, e.to_string()))?;
    // Checked again just before writing, as the sheet export does.
    solidcraft_io::vfs::check(path).map_err(|e| EngineError::Other(e.to_string()))?;
    solidcraft_io::write_atomic(Path::new(path), text.as_bytes(), false).map_err(|e| EngineError::Other(e.to_string()))?;
    let reply = TemplateSaveResult { path: path.into(), bytes: text.len(), param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

fn template_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let default = template::default_for(template::is_assembly(&s.doc));
    let entry = |name: &str| TemplateEntry {
        name: name.into(),
        title: template::title(name).unwrap_or_default(),
        assembly: template::assembly(name).unwrap_or_default(),
        default: name == default,
    };
    let templates = template::BUILT_INS.iter().map(|name| entry(name)).collect();
    serde_json::to_value(TemplateListResult { templates, param_warnings: Vec::new() }).map_err(|e| EngineError::Other(e.to_string()))
}

fn list(s: &mut Session, _p: &Value) -> Result<Value> {
    let listed: Vec<Value> = s
        .doc
        .drawings
        .iter()
        .map(|d| json!({"id": ids::encode(d.id), "name": d.name, "sheets": d.sheets.len(), "open": s.open_drawing == Some(d.id)}))
        .collect();
    Ok(json!({"drawings": listed}))
}

fn open(s: &mut Session, p: &Value) -> Result<Value> {
    s.open_drawing = Some(named("drawing.open", s, p)?);
    inspect(s, &Value::Null)
}

fn close(s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(json!({"closed": s.open_drawing.take().map(ids::encode)}))
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let id = named("drawing.delete", s, p)?;
    if s.open_drawing == Some(id) {
        s.open_drawing = None;
    }
    s.doc_mut().drawings.retain(|d| d.id != id);
    Ok(json!({"deleted": ids::encode(id)}))
}

/// What view `view` is called until views have saved names: which way it looks, or where it sits of
/// the view it is projected from (`Front`, `Right of Front`, `Above Right of Front`).
fn view_label(d: &Drawing, view: &View) -> String {
    let mut label = String::new();
    let mut at = view;
    for _ in 0..MAX_VIEW_DEPTH {
        match at.kind {
            ViewKind::Base { view: ViewDirection::Named(named), .. } => {
                let name = super::drawing_view::snake_name(named).replace('_', " ");
                let mut letters = name.chars();
                label.extend(letters.next().map(|first| first.to_ascii_uppercase()));
                label.push_str(letters.as_str());
                break;
            }
            ViewKind::Base { .. } => {
                label.push_str("Custom");
                break;
            }
            ViewKind::Projected { parent, side } => {
                label.push_str(match side {
                    ProjectedSide::Left => "Left of ",
                    ProjectedSide::Right => "Right of ",
                    ProjectedSide::Above => "Above ",
                    ProjectedSide::Below => "Below ",
                });
                let Some(next) = d.view(parent) else { break };
                at = next;
            }
        }
    }
    label
}

/// Sheet `sheet`'s views as `drawing.inspect` lists them: those with no parent on it by id, each
/// followed by the views projected from it. `dimensions` and `balloons` hold each view's, by its id.
fn sheet_views(d: &Drawing, sheet: u64, dimensions: &BTreeMap<u64, Vec<String>>, balloons: &BTreeMap<u64, Vec<String>>) -> Vec<Value> {
    let on: BTreeSet<u64> = d.views.iter().filter(|v| v.sheet == sheet).map(|v| v.id).collect();
    let mut first = Vec::new();
    let mut projected: BTreeMap<u64, Vec<&View>> = BTreeMap::new();
    for view in d.views.iter().filter(|v| on.contains(&v.id)) {
        match view.kind {
            ViewKind::Projected { parent, .. } if on.contains(&parent) => projected.entry(parent).or_default().push(view),
            _ => first.push(&**view),
        }
    }
    let mut stack: Vec<&View> = first.into_iter().rev().collect();
    let mut listed = Vec::with_capacity(on.len());
    while let Some(view) = stack.pop() {
        let mut entry = json!({
            "id": ids::encode(view.id),
            "kind": if matches!(view.kind, ViewKind::Base { .. }) { "base" } else { "projected" },
            "label": view_label(d, view),
            "dimensions": dimensions.get(&view.id).map_or(&[][..], Vec::as_slice),
            "balloons": balloons.get(&view.id).map_or(&[][..], Vec::as_slice),
        });
        if let ViewKind::Projected { parent, side } = view.kind {
            entry["parent"] = json!(ids::encode(parent));
            entry["side"] = json!(side);
        }
        listed.push(entry);
        if let Some(children) = projected.get(&view.id) {
            stack.extend(children.iter().rev());
        }
    }
    listed
}

/// The drawing open, as `drawing.inspect` answers, which making and opening one answer too.
fn inspect(s: &mut Session, _p: &Value) -> Result<Value> {
    let cmd = "drawing.inspect";
    let d = s.drawing(cmd)?;
    // One pass over each list for the whole drawing, however many sheets and views it has.
    let mut dimensions: BTreeMap<u64, Vec<String>> = BTreeMap::new();
    for dimension in &d.dimensions {
        dimensions.entry(dimension.view).or_default().push(ids::encode(dimension.id));
    }
    let mut balloons: BTreeMap<u64, Vec<String>> = BTreeMap::new();
    for balloon in &d.balloons {
        balloons.entry(balloon.view).or_default().push(ids::encode(balloon.id));
    }
    let state = s.model.state();
    // One tree for the drawing: each component it documents is weighed once.
    let mut tree = Tree::new(&s.doc, &state);
    let mut sheets = Vec::with_capacity(d.sheets.len());
    for (at, sheet) in d.sheets.iter().enumerate() {
        let size = sheet.sheet.size_mm().map_err(|e| bad(cmd, e.to_string()))?;
        sheets.push(json!({
            "id": ids::encode(sheet.id),
            "name": sheet.name,
            "size_mm": [size.x, size.y],
            "orientation": sheet.sheet.orientation,
            "title_block": sheet.title_block,
            "title_block_cells": title_block_cells(&s.doc, &mut tree, d, at)?,
            "title_block_overrides": sheet.title_block_overrides,
            "dpi": sheet.dpi,
            "line_group": sheet.line_group,
            "documents": sheet.documents.map(|c| json!({"source": ids::encode(c.source), "component": c.component})),
            "excluded": sheet.excluded,
            "views": sheet_views(d, sheet.id, &dimensions, &balloons),
            "tables": d.tables.iter().filter(|t| t.sheet == sheet.id).map(|t| ids::encode(t.id)).collect::<Vec<_>>(),
        }));
    }
    let parts_lists = parts_lists(d, &mut tree);
    Ok(json!({
        "id": ids::encode(d.id),
        "name": d.name,
        "units": d.units,
        "projection": d.projection,
        "updates": d.updates,
        "language": d.language,
        "font": d.font,
        "line_group": d.line_group,
        "font_note": solidcraft_sketch::paper_font_note(&solidcraft_sketch::font_dirs(), &d.font),
        "properties": d.properties,
        "sources": d.sources.iter().map(|source| json!({"id": ids::encode(source.id)})).collect::<Vec<_>>(),
        "sheets": sheets,
        "parts_lists": parts_lists,
        "revisions": {
            "numbering": d.revisions.numbering,
            "rows": super::drawing_revision::rows(&d.revisions),
            "next": d.revisions.next_revision(None),
            "issued": super::drawing_revision::issued(&d.revisions),
            "tags": super::drawing_revision::tags(&d.revisions),
        },
    }))
}

/// Each parts list with how many balloons point at each of its rows, its editor's ballooned column
/// (`drawing.inspect`): null for a list that can't be made, its component deleted or too large.
fn parts_lists(d: &Drawing, tree: &mut Tree<'_>) -> Vec<Value> {
    let reading = d.balloons_by_list();
    let lists = d.tables.iter().filter_map(|t| if let Kind::PartsList(list) = &t.kind { Some((t.id, list)) } else { None });
    lists
        .map(|(id, list)| {
            let ballooned = bom::list_with(tree, &list.request(), true).ok().map(|listed| {
                let rows = balloons_by_row(reading.get(&id).map_or(&[], Vec::as_slice), &listed, tree);
                rows.into_iter().map(|(item, balloons)| json!({"item": item, "balloons": balloons})).collect::<Vec<_>>()
            });
            json!({"table": ids::encode(id), "ballooned": ballooned})
        })
        .collect()
}

/// The text cells of sheet `at`'s title block as the sheet shows them (`drawing.inspect`), each
/// with what doesn't fit on paper or why its text is off the sheet.
fn title_block_cells(doc: &Document, tree: &mut Tree<'_>, d: &Drawing, at: usize) -> Result<Vec<Value>> {
    let shown = title_block_text::shown(doc, tree, d, at);
    // Laid out for its notes only: nothing is placed on a sheet here.
    // `Drawing::validate` checks the layout, so an error here is a drawing that skipped it.
    let block = match d.sheets.get(at) {
        Some(sheet) => sheet.title_block.as_ref().zip(sheet.title_block_layout()?),
        None => None,
    };
    let notes = block.map(|(block, placed)| title_block_text::lay(block, &placed, &d.font, &d.language, &shown).notes).unwrap_or_default();
    let note = |note: &Note| match note {
        Note::Fit(Warning::Chars { count, max }) => json!({"fit": "chars", "count": count, "max": max}),
        Note::Fit(Warning::Height) => json!({"fit": "height"}),
        Note::Fit(Warning::Width) => json!({"fit": "width"}),
        Note::Refused(why) => json!({"refused": why}),
    };
    let cell = |shown: &CellText| {
        let mut cell = json!({"name": shown.name, "text": shown.text});
        if let Some(value) = &shown.replaced {
            cell["value"] = json!(value);
        }
        let notes: Vec<Value> = notes.iter().filter(|(cell, _)| *cell == shown.name).map(|(_, n)| note(n)).collect();
        if !notes.is_empty() {
            cell["notes"] = json!(notes);
        }
        cell
    };
    Ok(shown.iter().map(cell).collect())
}

fn set(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.set";
    let request: DrawingSetParams = crate::schema::read(cmd, p)?;
    s.edit_drawing(cmd, |d| {
        if let Some(name) = request.name {
            d.name = name;
        }
        if let Some(language) = request.language {
            d.language = language;
        }
        if let Some(units) = request.units {
            d.units = units;
        }
        if let Some(projection) = request.projection {
            d.projection = projection;
        }
        if let Some(updates) = request.updates {
            d.updates = updates;
        }
        if let Some(group) = request.line_group {
            d.line_group = group;
        }
        if let Some(font) = font(cmd, request.font.as_deref())? {
            d.font = font.to_string();
        }
        if let Some(numbering) = request.revision_numbering {
            Arc::make_mut(&mut d.revisions).numbering = numbering;
        }
        Ok(())
    })?;
    inspect(s, &Value::Null)
}

fn properties(s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(super::metadata::reading(&s.drawing("drawing.properties")?.properties))
}

/// The open drawing with the properties `next`, kept when they changed: one undo step.
fn keep_properties(s: &mut Session, cmd: &str, next: solidcraft_doc::Metadata) -> Result<Value> {
    s.edit_drawing(cmd, |d| {
        d.properties = Arc::new(next);
        Ok(())
    })?;
    Ok(json!(s.drawing(cmd)?.properties))
}

fn properties_set(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.properties.set";
    let (next, changed) = super::metadata::setting(cmd, &s.drawing(cmd)?.properties, p)?;
    let metadata = keep_properties(s, cmd, next)?;
    Ok(json!({"metadata": metadata, "changed": changed}))
}

fn properties_remove(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.properties.remove";
    let (next, removed) = super::metadata::removing(cmd, &s.drawing(cmd)?.properties, p)?;
    let metadata = keep_properties(s, cmd, next)?;
    Ok(json!({"metadata": metadata, "removed": removed}))
}

#[cfg(test)]
#[path = "drawing_tests.rs"]
mod tests;
