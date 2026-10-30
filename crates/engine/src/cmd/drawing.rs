//! Drawing commands (#3890, K4): make, list, open, close and delete a design's drawings, and read
//! and change the open drawing's own settings. A drawing lives in its design's file
//! ([`solidcraft_doc::Document::drawings`]); while one is open, the drawing commands change it and
//! the model's wait ([`super::Part`]). Each change of a drawing is one undo step of that drawing,
//! and evaluates nothing; making and deleting one are steps of the model, whose list of drawings
//! they change.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_doc::bom::{Mass, Tree};
use solidcraft_doc::drawing::fields::{Component, Values};
use solidcraft_doc::drawing::{self, Drawing, Source, Units, template};
use solidcraft_doc::{Document, ids};
use solidcraft_geom::cell::{Content, Projection};
use solidcraft_geom::title_block::CellKind;

use super::CommandSpec;
use crate::params::{bad, str_};
use crate::schema::drawing::{SheetTemplateChange, TemplateApplyParams, TemplateApplyResult, TemplateEntry, TemplateListParams, TemplateListResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const TEMPLATE_APPLY_CONTRACT: TypedContract<TemplateApplyParams> = TypedContract::new();
const TEMPLATE_LIST_CONTRACT: TypedContract<TemplateListParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("drawing.create", "Create Drawing", create).any_part().icon("new").doc_only().params(
        "name?: the drawing's name (default: the design's); template?: a template's name (drawing.template.list; default iso-a3-part, or iso-a3-assembly for a design with components); language?: the drawing's content language, a BCP 47 tag (default: the template's, en for the built-ins); units?: \"follow_model\" or one length unit such as \"mm\" (default: the template's); projection?: first or third (angle) (default: the template's, third for the built-ins). Adds a drawing of this design made from the template, with ids of its own, and opens it (one undo step of the model) → as drawing.inspect",
    ),
    CommandSpec::new("drawing.template.apply", "Apply Drawing Template", crate::checked!("drawing.template.apply", template_apply))
        .in_drawing()
        .icon("settings")
        .doc_only()
        .contract(&TEMPLATE_APPLY_CONTRACT)
        .output(output_schema_for::<TemplateApplyResult>)
        .params(
            "template: a template's name (drawing.template.list); dry_run?: true to answer without changing anything. Each sheet takes the paper, drawing border and title block of the template's sheet in its place (its last for sheets past its own); text typed over a cell stays while the new block has a text cell of that name. Nothing moves → changed: [{sheet, paper, border, title_block, lost: [cells whose typed text is dropped]}] for each sheet that changes, outside: the views on a sheet whose new paper leaves them outside its drawing space (as drawing.sheet.set lists them), dry_run. One undo step, none when nothing changes.",
        ),
    CommandSpec::new("drawing.template.list", "List Drawing Templates", crate::checked!("drawing.template.list", template_list))
        .any_part()
        .icon("list")
        .noundo()
        .doc_only()
        .contract(&TEMPLATE_LIST_CONTRACT)
        .output(output_schema_for::<TemplateListResult>)
        .params(
            "→ templates: [{name, title, default: whether drawing.create starts from it for this design}], the built-ins: ISO A4 to A0 for a part and for an assembly",
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
        "→ {id, name, units, projection, language, properties, sources: [{id}], sheets: [{id, name, size_mm, orientation, title_block (its definition, or null), title_block_cells: [{name, text, value?}] (each text cell as the sheet shows it: its fields' values, \"-\" for one with none, or the text typed over it with the value it hides as `value`), title_block_overrides, dpi, documents, excluded}], revisions: {rows: [{row, revision, description, designer, checker, date, hidden}], the oldest first (the newest is the drawing's revision), next: the revision a new row gets}}. Read-only.",
    ),
    CommandSpec::new("drawing.set", "Drawing Settings", set)
        .in_drawing()
        .icon("settings")
        .doc_only()
        .params("name?, language?: a BCP 47 tag, units?: \"follow_model\" or one length unit such as \"mm\", projection?: first or third (angle), which every projected view follows. One undo step."),
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

/// `units` as given: `follow_model`, or a length unit the drawing's rules check.
fn units(cmd: &str, p: &Value) -> Result<Option<Units>> {
    match p.get("units") {
        None => Ok(None),
        Some(Value::String(u)) if u == "follow_model" => Ok(Some(Units::FollowModel)),
        Some(Value::String(u)) => Ok(Some(Units::Fixed { unit: u.clone() })),
        Some(_) => Err(bad(cmd, "`units` is \"follow_model\" or a length unit such as \"mm\"")),
    }
}

/// `projection` as given: first or third angle.
fn projection(cmd: &str, p: &Value) -> Result<Option<Projection>> {
    match p.get("projection") {
        None => Ok(None),
        Some(v) => serde_json::from_value(v.clone()).map(Some).map_err(|_| bad(cmd, "`projection` is first or third (angle)")),
    }
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
    let name = text(cmd, p, "name")?.map_or_else(|| s.doc.name.clone(), str::to_string);
    let fail = |e: solidcraft_doc::DocError| bad(cmd, e.to_string());
    let from = text(cmd, p, "template")?.unwrap_or_else(|| template::default_name(&s.doc));
    let unknown = || bad(cmd, format!("no template `{from}` (drawing.template.list lists them)"));
    let chosen = template::built_in(from).ok_or_else(unknown)?.map_err(fail)?;
    // Not the id of a drawing undo or redo could bring back either.
    let steps = s.undo.iter().chain(&s.redo).filter_map(|step| step.part);
    let taken: BTreeSet<u64> = s.doc.drawings.iter().map(|d| d.id).chain(steps).collect();
    let mut d = template::instantiate(&chosen, &name, &taken, &mut s.ids).map_err(fail)?;
    if let Some(language) = text(cmd, p, "language")? {
        d.language = language.to_string();
    }
    if let Some(projection) = projection(cmd, p)? {
        d.projection = projection;
    }
    let source = d.allocate_id(&mut s.ids).map_err(fail)?;
    d.sources.push(Source { id: source });
    if let Some(units) = units(cmd, p)? {
        d.units = units;
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
    let unknown = || bad(cmd, format!("no template `{from}` (drawing.template.list lists them)"));
    let chosen = template::built_in(from).ok_or_else(unknown)?.map_err(fail)?;
    let dry_run = match p.get("dry_run") {
        None => false,
        Some(Value::Bool(dry_run)) => *dry_run,
        Some(_) => return Err(bad(cmd, "`dry_run` is true or false")),
    };
    let (applied, changed) = template::apply(s.drawing(cmd)?, &chosen).map_err(fail)?;
    // The views on a sheet whose paper changed: listed when they fall outside, never moved.
    let views: Vec<u64> = changed.iter().filter(|c| c.paper).flat_map(|c| applied.on_sheet(c.sheet).views).collect();
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

fn template_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let default = template::default_name(&s.doc);
    let templates = template::BUILT_INS
        .iter()
        .map(|&name| TemplateEntry { name: name.into(), title: template::title(name).unwrap_or_default(), default: name == default })
        .collect();
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

/// The drawing open, as `drawing.inspect` answers, which making and opening one answer too.
fn inspect(s: &mut Session, _p: &Value) -> Result<Value> {
    let cmd = "drawing.inspect";
    let d = s.drawing(cmd)?;
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
            "title_block_cells": title_block_cells(&s.doc, &mut tree, d, at),
            "title_block_overrides": sheet.title_block_overrides,
            "dpi": sheet.dpi,
            "documents": sheet.documents.map(|c| json!({"source": ids::encode(c.source), "component": c.component})),
            "excluded": sheet.excluded,
        }));
    }
    Ok(json!({
        "id": ids::encode(d.id),
        "name": d.name,
        "units": d.units,
        "projection": d.projection,
        "language": d.language,
        "properties": d.properties,
        "sources": d.sources.iter().map(|source| json!({"id": ids::encode(source.id)})).collect::<Vec<_>>(),
        "sheets": sheets,
        "revisions": {"rows": super::drawing_revision::rows(&d.revisions), "next": d.revisions.next_revision(None)},
    }))
}

/// The text cells of sheet `at`'s title block as the sheet shows them (`drawing.inspect`).
fn title_block_cells(doc: &Document, tree: &mut Tree<'_>, d: &Drawing, at: usize) -> Vec<Value> {
    let Some(sheet) = d.sheets.get(at) else { return Vec::new() };
    let Some(block) = &sheet.title_block else { return Vec::new() };
    // What the model says of the component it documents: a component since deleted has nothing.
    let id = sheet.documents.map(|documents| documents.component);
    let (mass, materials) = id.and_then(|id| tree.component(id).ok()).unzip();
    let mass = mass.and_then(|mass| if let Mass::Grams(grams) = mass { Some(format_mass(grams)) } else { None });
    let material = materials.filter(|m| !m.is_empty()).map(|m| m.join("; "));
    let component = id.and_then(|id| documented(doc, id, material.as_deref(), mass.as_deref()));
    let values = Values { drawing: d, sheet: at, component };
    let cell = |name: &str, content: &Content| {
        let value = values.text(content);
        match sheet.title_block_overrides.get(name) {
            Some(typed) => json!({"name": name, "text": typed, "value": value}),
            None => json!({"name": name, "text": value}),
        }
    };
    block.cells.iter().filter_map(|c| if let CellKind::Text(text) = &c.kind { Some(cell(&c.name, &text.content)) } else { None }).collect()
}

/// Component `id` of the design (0: the design itself) as a title block's fields read it, with
/// what the model says of it; none when the design has no such component.
fn documented<'a>(doc: &'a Document, id: u64, material: Option<&'a str>, mass: Option<&'a str>) -> Option<Component<'a>> {
    if id == 0 {
        return Some(Component { name: &doc.name, metadata: &doc.metadata, properties: None, material, mass });
    }
    let c = doc.components.iter().find(|c| c.id == id)?;
    Some(Component { name: &c.name, metadata: &c.metadata, properties: Some(&c.properties), material, mass })
}

/// A mass as a sheet shows it: kilograms to two places, until #4867 gives drawings their units.
fn format_mass(grams: f64) -> String {
    format!("{:.2} kg", grams / 1000.0)
}

fn set(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.set";
    s.edit_drawing(cmd, |d| {
        if let Some(name) = text(cmd, p, "name")? {
            d.name = name.to_string();
        }
        if let Some(language) = text(cmd, p, "language")? {
            d.language = language.to_string();
        }
        if let Some(units) = units(cmd, p)? {
            d.units = units;
        }
        if let Some(projection) = projection(cmd, p)? {
            d.projection = projection;
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
