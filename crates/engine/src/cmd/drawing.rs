//! Drawing commands (#3890, K4): make, list, open, close and delete a design's drawings, and read
//! and change the open drawing's own settings. A drawing lives in its design's file
//! ([`solidcraft_doc::Document::drawings`]); while one is open, the drawing commands change it and
//! the model's wait ([`super::Part`]). Each change of a drawing is one undo step of that drawing,
//! and evaluates nothing; making and deleting one are steps of the model, whose list of drawings
//! they change.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_doc::drawing::view::{MAX_VIEW_DEPTH, View, ViewKind};
use solidcraft_doc::drawing::{self, Drawing, Source, Units};
use solidcraft_doc::drawing_view::ViewDirection;
use solidcraft_doc::ids;
use solidcraft_geom::cell::Projection;
use solidcraft_geom::drawing_sheet::{DrawingBorderProfile, Orientation, PaperSize, Sheet};
use solidcraft_geom::drawing_view::ProjectedSide;
use solidcraft_geom::title_block;

use super::CommandSpec;
use crate::params::{bad, str_};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("drawing.create", "Create Drawing", create).any_part().icon("new").doc_only().params(
        "name?: the drawing's name (default: the design's); language?: the drawing's content language, a BCP 47 tag (default en); units?: \"follow_model\" (default) or one length unit such as \"mm\"; font?: the font family its text is set in, by name (default: the built-in font). Adds a drawing of this design, one ISO A3 landscape sheet with the ISO border and the built-in title block, and opens it (one undo step of the model) → as drawing.inspect",
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
        "→ {id, name, units, projection, language, font, font_note: why its text is set in the built-in font instead (the font isn't on this computer, or can't be set on paper), or null, properties, sources: [{id}], sheets: [{id, name, size_mm, orientation, title_block (its definition, or null), dpi, documents, excluded, views: [{id, kind: base|projected, parent?, side?, label, dimensions: [ids], balloons: [ids]}], base views by id, each followed by those projected from it}], revisions: {rows: [{row, revision, description, designer, checker, date, hidden}], the oldest first (the newest is the drawing's revision), next: the revision a new row gets}}. Read-only.",
    ),
    CommandSpec::new("drawing.set", "Drawing Settings", set)
        .in_drawing()
        .icon("settings")
        .doc_only()
        .params("name?, language?: a BCP 47 tag, units?: \"follow_model\" or one length unit such as \"mm\", projection?: first or third (angle), which every projected view follows, font?: the font family its text is set in, by name, never a file (\"\": the built-in font). One undo step."),
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

/// `font` as given: a font family's name, never a file (drawings name fonts by family, so they
/// travel).
fn font<'a>(cmd: &str, p: &'a Value) -> Result<Option<&'a str>> {
    let font = text(cmd, p, "font")?;
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
    let name = text(cmd, p, "name")?.map_or_else(|| s.doc.name.clone(), str::to_string);
    let language = text(cmd, p, "language")?.unwrap_or("en");
    // Not the id of a drawing undo or redo could bring back either.
    let steps = s.undo.iter().chain(&s.redo).filter_map(|step| step.part);
    let taken: BTreeSet<u64> = s.doc.drawings.iter().map(|d| d.id).chain(steps).collect();
    let a3 = Sheet::standard(PaperSize::A3, Orientation::Landscape);
    let fail = |e: solidcraft_doc::DocError| bad(cmd, e.to_string());
    let mut d = Drawing::new(&name, language, "Sheet 1", a3, Some(DrawingBorderProfile::iso()), &taken, &mut s.ids).map_err(fail)?;
    if let Some(first) = d.sheets.first_mut() {
        Arc::make_mut(first).title_block = Some(title_block::default_block());
    }
    let source = d.allocate_id(&mut s.ids).map_err(fail)?;
    d.sources.push(Source { id: source });
    if let Some(units) = units(cmd, p)? {
        d.units = units;
    }
    if let Some(font) = font(cmd, p)? {
        d.font = font.to_string();
    }
    d.validate().map_err(fail)?;
    s.open_drawing = Some(d.id);
    s.doc_mut().put_drawing(Arc::new(d));
    inspect(s, &Value::Null)
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
    let mut sheets = Vec::with_capacity(d.sheets.len());
    for sheet in &d.sheets {
        let size = sheet.sheet.size_mm().map_err(|e| bad(cmd, e.to_string()))?;
        sheets.push(json!({
            "id": ids::encode(sheet.id),
            "name": sheet.name,
            "size_mm": [size.x, size.y],
            "orientation": sheet.sheet.orientation,
            "title_block": sheet.title_block,
            "dpi": sheet.dpi,
            "documents": sheet.documents.map(|c| json!({"source": ids::encode(c.source), "component": c.component})),
            "excluded": sheet.excluded,
            "views": sheet_views(d, sheet.id, &dimensions, &balloons),
        }));
    }
    Ok(json!({
        "id": ids::encode(d.id),
        "name": d.name,
        "units": d.units,
        "projection": d.projection,
        "language": d.language,
        "font": d.font,
        "font_note": solidcraft_sketch::paper_font_note(&solidcraft_sketch::font_dirs(), &d.font),
        "properties": d.properties,
        "sources": d.sources.iter().map(|source| json!({"id": ids::encode(source.id)})).collect::<Vec<_>>(),
        "sheets": sheets,
        "revisions": {"rows": super::drawing_revision::rows(&d.revisions), "next": d.revisions.next_revision(None)},
    }))
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
        if let Some(font) = font(cmd, p)? {
            d.font = font.to_string();
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
