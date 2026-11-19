//! Saved fills use complete sheet-sketch boundaries and drawing-owned pattern snapshots.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use solidcore_record::{Region2, hatch};
use solidcraft_doc::drawing::{Drawing, FillPaint, FillRegionStatus, PatternSnapshot, ResolvedSketchFill, SketchFill};
use solidcraft_doc::ids;

use super::CommandSpec;
use crate::params::bad;
use crate::schema::drawing_fill as contract;
use crate::schema::output_schema_for;
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("drawing.sketch.fill", "Fill Sheet Sketch", crate::checked!("drawing.sketch.fill", create)).begins_edit()
        .in_drawing().doc_only().icon("sketch").contract(&contract::CREATE).output(output_schema_for::<contract::Readings>)
        .params("sketch: exact sheet-sketch id; boundary: complete canonical outer and holes curve identities; paint: {kind: solid} or {kind: hatch, pattern: exact pattern id, pen_width_mm}. Saves one fill in paper millimetres, in one drawing undo step."),
    CommandSpec::new("drawing.fill.edit", "Edit Sheet Fill", crate::checked!("drawing.fill.edit", edit)).begins_edit_when(crate::edit_start::changes_editor_fields)
        .in_drawing().doc_only().icon("sketch").contract(&contract::EDIT).output(output_schema_for::<contract::Readings>)
        .params("fill: exact fill id; boundary?, paint?: replacements. A replacement boundary must match one complete current profile. Unchanged edits add no undo step."),
    CommandSpec::new("drawing.fill.delete", "Delete Sheet Fill", crate::checked!("drawing.fill.delete", delete))
        .in_drawing().doc_only().icon("delete").contract(&contract::DELETE).output(output_schema_for::<contract::DeletedFill>)
        .params("fill: exact fill id. Removes that fill in one drawing undo step."),
    CommandSpec::new("drawing.fill.inspect", "Inspect Sheet Fill", crate::checked!("drawing.fill.inspect", inspect))
        .in_drawing().doc_only().noundo().icon("inspect").contract(&contract::INSPECT).output(output_schema_for::<contract::Readings>)
        .params("fill?: exact fill id, else all fills in the open drawing. Reports complete boundary identity, paint and current, lost-boundary or failed status without changing saved geometry."),
    CommandSpec::new("drawing.fill.profiles", "List Fill Profiles", crate::checked!("drawing.fill.profiles", profiles))
        .in_drawing().doc_only().noundo().icon("inspect").contract(&contract::PROFILES).output(output_schema_for::<contract::ProfileChoices>)
        .params("sketch: exact sheet-sketch id. Lists certified complete outer-and-hole curve identities and counts uncertified profiles. Incomplete or inexact source conversion refuses. Read-only; ordinary sketch inspection remains available separately."),
    CommandSpec::new("drawing.pattern.import", "Import Fill Pattern", crate::checked!("drawing.pattern.import", import))
        .in_drawing().doc_only().icon("import").contract(&contract::PATTERN_IMPORT).output(output_schema_for::<contract::Patterns>)
        .params("kind: pat with source, units?: mm|inches, scale?: positive multiplier; or kind: builtin with pattern: lines_45|cross_hatch. Stores independent pattern snapshots. Duplicate names retain distinct ids. Import is complete or refused, in one drawing undo step."),
    CommandSpec::new("drawing.pattern.inspect", "Inspect Fill Pattern", crate::checked!("drawing.pattern.inspect", inspect_pattern))
        .in_drawing().doc_only().noundo().icon("inspect").contract(&contract::PATTERN_INSPECT).output(output_schema_for::<contract::Patterns>)
        .params("pattern?: exact pattern id, else all snapshots in the open drawing. Lengths are paper millimetres; angles are radians; signed gaps and zero dots remain explicit."),
    CommandSpec::new("drawing.pattern.delete", "Delete Fill Pattern", crate::checked!("drawing.pattern.delete", delete_pattern))
        .in_drawing().doc_only().icon("delete").contract(&contract::PATTERN_DELETE).output(output_schema_for::<contract::DeletedPattern>)
        .params("pattern: exact pattern id. Refuses while any fill references it. Removes an unused snapshot in one drawing undo step."),
];

fn id(cmd: &str, text: &str) -> Result<u64> {
    ids::decode(text).ok_or_else(|| bad(cmd, "an exact encoded drawing object id is required"))
}

fn reply(cmd: &str, value: impl Serialize) -> Result<Value> {
    serde_json::to_value(value).map_err(|error| bad(cmd, error.to_string()))
}

fn next_id(s: &mut Session, drawing: &Drawing, taken: &mut BTreeSet<u64>, cmd: &str) -> Result<u64> {
    if taken.is_empty() {
        taken.extend(drawing.object_ids());
        for step in s.undo.iter().chain(&s.redo) {
            if let Some(previous) = step.doc.drawing(drawing.id) {
                taken.extend(previous.object_ids());
            }
        }
    }
    let id = s.ids.next(taken).map_err(|error| bad(cmd, error.to_string()))?;
    taken.insert(id);
    Ok(id)
}

fn paint(cmd: &str, value: contract::Paint) -> Result<FillPaint> {
    Ok(match value {
        contract::Paint::Solid => FillPaint::Solid,
        contract::Paint::Hatch { pattern, pen_width_mm } => FillPaint::Hatch { pattern: id(cmd, &pattern)?, pen_width_mm },
    })
}

fn paint_reading(paint: &FillPaint) -> contract::Paint {
    match paint {
        FillPaint::Solid => contract::Paint::Solid,
        FillPaint::Hatch { pattern, pen_width_mm } => contract::Paint::Hatch { pattern: ids::encode(*pattern), pen_width_mm: *pen_width_mm },
    }
}

fn admit_paint(drawing: &Drawing, id: u64, region: Region2, cmd: &str) -> Result<()> {
    let fill = drawing.sheet_fill(id).ok_or_else(|| bad(cmd, "the fill disappeared before paint admission"))?;
    let sheet = drawing.sheet_sketch(fill.sketch).ok_or_else(|| bad(cmd, "the owning sheet sketch disappeared"))?.sheet;
    let resolved = ResolvedSketchFill {
        drawing: drawing.id,
        sheet,
        id,
        sketch: fill.sketch,
        region,
        paint: fill.paint.clone(),
        status: FillRegionStatus::Current,
    };
    crate::drawing_scene::fill::paper_group(drawing, &resolved).map(|_| ()).map_err(|error| bad(cmd, error.to_string()))
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.sketch.fill";
    let p: contract::Create = crate::schema::read(cmd, p)?;
    let drawing = s.drawing(cmd)?.clone();
    let sketch = id(cmd, &p.sketch)?;
    let boundary = p.boundary.into();
    let last_region = drawing.sheet_fill_region(&s.doc, sketch, &boundary).map_err(|error| bad(cmd, error.to_string()))?;
    let paint = paint(cmd, p.paint)?;
    let region = last_region.clone();
    let fill = next_id(s, &drawing, &mut BTreeSet::new(), cmd)?;
    s.edit_drawing(cmd, |drawing| {
        drawing.fills.push(Arc::new(SketchFill { id: fill, sketch, boundary, paint, last_region }));
        drawing.fills.sort_by_key(|record| record.id);
        admit_paint(drawing, fill, region, cmd)
    })?;
    readings(s, Some(fill), cmd)
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.fill.edit";
    let p: contract::Edit = crate::schema::read(cmd, p)?;
    let fill = id(cmd, &p.fill)?;
    let drawing = s.drawing(cmd)?.clone();
    let original = drawing.sheet_fill(fill).ok_or_else(|| bad(cmd, "the open drawing has no such fill"))?;
    let mut edited = original.as_ref().clone();
    if let Some(boundary) = p.boundary {
        edited.boundary = boundary.into();
        edited.last_region = drawing.sheet_fill_region(&s.doc, edited.sketch, &edited.boundary).map_err(|error| bad(cmd, error.to_string()))?;
    }
    if let Some(value) = p.paint {
        edited.paint = paint(cmd, value)?;
    }
    let resolved = drawing.resolve_sheet_fill(&s.doc, fill).map_err(|error| bad(cmd, error.to_string()))?;
    let region = if edited.boundary == original.boundary { resolved.region } else { edited.last_region.clone() };
    s.edit_drawing(cmd, |drawing| {
        let slot = drawing.fills.iter_mut().find(|record| record.id == fill).ok_or_else(|| bad(cmd, "the fill changed while editing"))?;
        *slot = Arc::new(edited);
        admit_paint(drawing, fill, region, cmd)
    })?;
    readings(s, Some(fill), cmd)
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.fill.delete";
    let p: contract::FillId = crate::schema::read(cmd, p)?;
    let fill = id(cmd, &p.fill)?;
    let drawing = s.drawing(cmd)?.id;
    s.edit_drawing(cmd, |drawing| {
        let index = drawing.fills.iter().position(|record| record.id == fill).ok_or_else(|| bad(cmd, "the open drawing has no such fill"))?;
        drawing.fills.remove(index);
        Ok(())
    })?;
    reply(cmd, contract::DeletedFill { drawing: ids::encode(drawing), fill: ids::encode(fill) })
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.fill.inspect";
    let p: contract::Inspect = crate::schema::read(cmd, p)?;
    readings(s, p.fill.as_deref().map(|text| id(cmd, text)).transpose()?, cmd)
}

fn profiles(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.fill.profiles";
    let p: contract::ProfileParams = crate::schema::read(cmd, p)?;
    let sketch = id(cmd, &p.sketch)?;
    let drawing = s.drawing(cmd)?;
    let profiles = drawing.sheet_fill_profiles(&s.doc, sketch).map_err(|error| bad(cmd, error.to_string()))?;
    reply(
        cmd,
        contract::ProfileChoices {
            drawing: ids::encode(drawing.id),
            sketch: ids::encode(sketch),
            fill_profiles: contract::Profiles {
                boundaries: profiles.profiles.into_iter().map(|profile| profile.boundary.into()).collect(),
                uncertified: profiles.uncertified,
            },
        },
    )
}

fn readings(s: &Session, selected: Option<u64>, cmd: &str) -> Result<Value> {
    let drawing = s.drawing(cmd)?;
    if selected.is_some_and(|id| drawing.sheet_fill(id).is_none()) {
        return Err(bad(cmd, "the open drawing has no such fill"));
    }
    let mut fills = Vec::new();
    let mut batches = BTreeMap::new();
    let mut records: Vec<_> = drawing.fills.iter().filter(|record| selected.is_none_or(|id| record.id == id)).collect();
    records.sort_by_key(|record| record.id);
    for fill in records {
        let batch =
            batches.entry(fill.sketch).or_insert_with(|| drawing.resolve_sheet_fill_regions(&s.doc, fill.sketch).map_err(|error| error.to_string()));
        let (status, reason) = match batch {
            Ok(regions) => {
                let resolved = regions.iter().find(|region| region.id == fill.id).ok_or_else(|| bad(cmd, "the selected fill disappeared"))?;
                match &resolved.status {
                    FillRegionStatus::Current => (contract::Status::Current, None),
                    FillRegionStatus::LostBoundary(reason) => (contract::Status::LostBoundary, Some(reason.clone())),
                }
            }
            Err(reason) => (contract::Status::Failed, Some(reason.clone())),
        };
        let sheet = drawing.sheet_sketch(fill.sketch).ok_or_else(|| bad(cmd, "the owning sketch disappeared"))?.sheet;
        fills.push(contract::Reading {
            fill: ids::encode(fill.id),
            sheet: ids::encode(sheet),
            sketch: ids::encode(fill.sketch),
            boundary: fill.boundary.clone().into(),
            paint: paint_reading(&fill.paint),
            status,
            reason,
        });
    }
    reply(cmd, contract::Readings { drawing: ids::encode(drawing.id), fills })
}

fn pattern_reading(record: &PatternSnapshot) -> contract::PatternReading {
    contract::PatternReading {
        id: ids::encode(record.id),
        name: record.pattern.name.clone(),
        description: record.pattern.description.clone(),
        families: record
            .pattern
            .families
            .iter()
            .map(|family| contract::Family {
                angle: family.angle,
                origin: [family.origin.x, family.origin.y],
                delta_x: family.delta_x,
                delta_y: family.delta_y,
                dashes: family.dashes.clone(),
            })
            .collect(),
    }
}

fn import(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.pattern.import";
    let p: contract::PatternImport = crate::schema::read(cmd, p)?;
    let patterns = match p {
        contract::PatternImport::Pat { source, units, scale } => solidcraft_io::read_pat(
            source.as_bytes(),
            solidcraft_io::PatImportOptions {
                units: match units {
                    contract::Units::Mm => solidcraft_io::PatUnits::Millimetres,
                    contract::Units::Inches => solidcraft_io::PatUnits::Inches,
                },
                scale,
            },
        )
        .map_err(|error| bad(cmd, error.to_string()))?,
        contract::PatternImport::Builtin { pattern } => vec![match pattern {
            contract::Builtin::Lines45 => hatch::Pattern::lines_45(),
            contract::Builtin::CrossHatch => hatch::Pattern::cross_hatch(),
        }],
    };
    let drawing = s.drawing(cmd)?.clone();
    let mut taken = BTreeSet::new();
    let mut records = Vec::new();
    for pattern in patterns {
        records.push(Arc::new(PatternSnapshot { id: next_id(s, &drawing, &mut taken, cmd)?, pattern }));
    }
    let result = contract::Patterns { drawing: ids::encode(drawing.id), patterns: records.iter().map(|record| pattern_reading(record)).collect() };
    s.edit_drawing(cmd, |drawing| {
        drawing.patterns.extend(records);
        drawing.patterns.sort_by_key(|record| record.id);
        Ok(())
    })?;
    reply(cmd, result)
}

fn inspect_pattern(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.pattern.inspect";
    let p: contract::PatternInspect = crate::schema::read(cmd, p)?;
    let selected = p.pattern.as_deref().map(|text| id(cmd, text)).transpose()?;
    let drawing = s.drawing(cmd)?;
    if selected.is_some_and(|id| drawing.hatch_pattern(id).is_none()) {
        return Err(bad(cmd, "the open drawing has no such pattern"));
    }
    let mut records: Vec<_> = drawing.patterns.iter().filter(|record| selected.is_none_or(|id| record.id == id)).collect();
    records.sort_by_key(|record| record.id);
    let patterns = records.into_iter().map(|record| pattern_reading(record)).collect();
    reply(cmd, contract::Patterns { drawing: ids::encode(drawing.id), patterns })
}

fn delete_pattern(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.pattern.delete";
    let p: contract::PatternId = crate::schema::read(cmd, p)?;
    let pattern = id(cmd, &p.pattern)?;
    let drawing = s.drawing(cmd)?.id;
    s.edit_drawing(cmd, |drawing| {
        if drawing.fills.iter().any(|fill| matches!(fill.paint, FillPaint::Hatch { pattern: id, .. } if id == pattern)) {
            return Err(bad(cmd, "the pattern is still referenced by a fill"));
        }
        let index =
            drawing.patterns.iter().position(|record| record.id == pattern).ok_or_else(|| bad(cmd, "the open drawing has no such pattern"))?;
        drawing.patterns.remove(index);
        Ok(())
    })?;
    reply(cmd, contract::DeletedPattern { drawing: ids::encode(drawing), pattern: ids::encode(pattern) })
}
