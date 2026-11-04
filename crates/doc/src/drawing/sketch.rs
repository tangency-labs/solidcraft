//! W38a's first record slice: sheet-owned, paper-mm geometry, with no model evaluation.
//! The Sketch payload stays the shared type. Links, patterns and offsets await the
//! receiver-aware store: refusing them here is explicit, never silent data loss.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;
use solidcraft_geom::Vec2;
use solidcraft_sketch::{ConstraintKind, CurveKind, MAX_CONSTRAINTS, MAX_COORD, MAX_CURVES, MAX_POINTS, MAX_SPLINE_POINTS, Sketch};

use super::{Drawing, MAX_DRAWING_BYTES, check_name, require};
use crate::{DocError, Result};

/// Maximum sheet sketches in one drawing; independently of the number of sheets.
pub const MAX_SHEET_SKETCHES: usize = 1000;
// One drawing cannot multiply the shared sketch array caps by its sketch count.
const MAX_ENTITIES: usize = MAX_POINTS + MAX_CURVES + MAX_CONSTRAINTS;
const MAX_VALIDATION_WORK: usize = 2_000_000;
const MAX_STORED_TEXT_BYTES: usize = 4096;
const MAX_LOCAL_ID_BYTES: usize = 512;
const AUXILIARY: [&str; 5] = ["links", "patterns", "offsets", "freeform_offsets", "wires"];

/// Authored sheet geometry. Points and lengths are paper XY millimetres, regardless
/// of the drawing's display units. The origin is the sheet's lower-left corner,
/// with x right and y up, not the border corner. No view scale or source path.
/// Expressions are retained verbatim; interpreting them is the later read-only store's job.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheetSketch {
    #[serde(with = "crate::ids")]
    pub id: u64,
    #[serde(with = "crate::ids")]
    pub sheet: u64,
    pub name: String,
    pub sketch: Sketch,
}

fn invalid(message: impl Into<String>) -> DocError {
    DocError::Invalid(format!("sheet sketch: {}", message.into()))
}

fn charge(used: &mut usize, amount: usize, limit: usize, what: &str) -> Result<()> {
    *used = used.checked_add(amount).filter(|n| *n <= limit).ok_or_else(|| invalid(format!("{what} limit exceeded")))?;
    Ok(())
}

fn text(s: &str) -> Result<()> {
    require(!s.trim().is_empty() && s.len() <= MAX_STORED_TEXT_BYTES && !s.chars().any(char::is_control), || {
        "sheet sketch expression is empty, too long or contains control characters".into()
    })
}

fn position(p: Vec2) -> Result<()> {
    require(p.is_finite() && p.x.abs() < MAX_COORD && p.y.abs() < MAX_COORD, || "sheet sketch point is out of range".into())
}

fn identity<'a>(seen: &mut BTreeSet<&'a str>, id: &'a str) -> Result<()> {
    require(!id.trim().is_empty() && id.len() <= MAX_LOCAL_ID_BYTES && !id.chars().any(char::is_control), || {
        "sheet sketch local id is empty, too long or contains control characters".into()
    })?;
    require(seen.insert(id), || "sheet sketch local ids must be unique".into())
}

/// Every collection is bounded before serde allocates the typed sketch payload.
/// A raw JSON Value already exists here; the drawing reader separately caps encoded bytes.
pub(super) fn check_raw(value: &Value) -> Result<()> {
    let Some(records) = value.get("sketches") else { return Ok(()) };
    let records = records.as_array().ok_or_else(|| invalid("sketches must be an array"))?;
    require(records.len() <= MAX_SHEET_SKETCHES, || "too many sheet sketches".into())?;
    let mut total = 0;
    let mut nodes = 0;
    let mut bytes = 0;
    for record in records {
        raw_tree(record, 0, &mut nodes, &mut bytes)?;
        let Some(sketch) = record.get("sketch") else { continue };
        for (key, limit) in [("points", MAX_POINTS), ("curves", MAX_CURVES), ("constraints", MAX_CONSTRAINTS)] {
            if let Some(array) = sketch.get(key) {
                let array = array.as_array().ok_or_else(|| invalid(format!("{key} must be an array")))?;
                require(array.len() <= limit, || format!("sheet sketch {key} limit exceeded"))?;
                charge(&mut total, array.len(), MAX_ENTITIES, "drawing sketch entity")?;
            }
        }
        for key in AUXILIARY {
            if let Some(array) = sketch.get(key) {
                let array = array.as_array().ok_or_else(|| invalid(format!("{key} must be an array")))?;
                require(array.is_empty(), || format!("not supported yet: sheet sketch {key}"))?;
            }
        }
        if let Some(curves) = sketch.get("curves").and_then(Value::as_array) {
            for curve in curves {
                if let Some(points) = curve.get("pts").and_then(Value::as_array) {
                    require(points.len() <= MAX_SPLINE_POINTS, || "sheet sketch spline point limit exceeded".into())?;
                    charge(&mut total, points.len(), MAX_ENTITIES, "drawing sketch entity")?;
                }
            }
        }
        if let Some(counters) = sketch.get("counters").and_then(Value::as_object) {
            require(counters.len() <= 64, || "sheet sketch counter prefix limit exceeded".into())?;
            require(counters.keys().all(|key| key.len() <= MAX_LOCAL_ID_BYTES), || "sheet sketch counter prefix too long".into())?;
        }
    }
    Ok(())
}

// These guards apply to from_value as well as from_json, and precede typed decoding.
fn raw_tree(value: &Value, depth: usize, nodes: &mut usize, bytes: &mut usize) -> Result<()> {
    require(depth <= 32, || "sheet sketch payload nested too deeply".into())?;
    charge(nodes, 1, MAX_ENTITIES * 16, "raw node")?;
    match value {
        Value::String(s) => {
            require(s.len() <= MAX_STORED_TEXT_BYTES, || "sheet sketch stored text too long".into())?;
            charge(bytes, s.len(), MAX_DRAWING_BYTES, "stored text")?;
        }
        Value::Array(array) => {
            require(array.len() <= MAX_ENTITIES, || "sheet sketch raw array too large".into())?;
            for item in array {
                raw_tree(item, depth + 1, nodes, bytes)?;
            }
        }
        Value::Object(object) => {
            require(object.len() <= MAX_ENTITIES, || "sheet sketch raw object too large".into())?;
            for (key, item) in object {
                require(key.len() <= MAX_LOCAL_ID_BYTES, || "sheet sketch field name too long".into())?;
                charge(bytes, key.len(), MAX_DRAWING_BYTES, "stored text")?;
                raw_tree(item, depth + 1, nodes, bytes)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Cheap cumulative admission precedes validation/solving of any candidate. It
/// counts unchanged records too: editing one record cannot exceed a drawing cap.
fn check_budget<'a>(records: impl Iterator<Item = &'a SheetSketch>) -> Result<()> {
    let mut entities = 0;
    let mut work = 0;
    for record in records {
        let sk = &record.sketch;
        for (count, cap) in [(sk.points.len(), MAX_POINTS), (sk.curves.len(), MAX_CURVES), (sk.constraints.len(), MAX_CONSTRAINTS)] {
            require(count <= cap, || "sheet sketch array limit exceeded".into())?;
            charge(&mut entities, count, MAX_ENTITIES, "drawing sketch entity")?;
        }
        for curve in &sk.curves {
            if let CurveKind::Spline { pts, .. } = &curve.kind {
                require(pts.len() <= MAX_SPLINE_POINTS, || "sheet sketch spline point limit exceeded".into())?;
                charge(&mut entities, pts.len(), MAX_ENTITIES, "drawing sketch entity")?;
            }
        }
        for constraint in &sk.constraints {
            // The shared slot validator can search the list twice. Keep the same
            // conservative work charge, before invoking it, even on staged edits.
            let cost = if matches!(constraint.kind, ConstraintKind::SlotOverallLength { .. }) {
                sk.constraints
                    .len()
                    .checked_mul(sk.constraints.len())
                    .and_then(|n| n.checked_mul(4))
                    .ok_or_else(|| invalid("validation work overflow"))?
            } else {
                1
            };
            charge(&mut work, cost, MAX_VALIDATION_WORK, "validation work")?;
        }
    }
    Ok(())
}

pub(super) fn check_drawing(drawing: &Drawing) -> Result<()> {
    require(drawing.sketches.len() <= MAX_SHEET_SKETCHES, || "too many sheet sketches".into())?;
    check_budget(drawing.sketches.iter().map(AsRef::as_ref))?;
    for record in &drawing.sketches {
        check_payload(record)?;
    }
    Ok(())
}

// Only after check_budget: the single payload is checked before and after the
// solver, while the Session's publication boundary checks the complete drawing.
fn check_payload(record: &SheetSketch) -> Result<()> {
    check_name(&record.name, "sheet sketch")?;
    let sk = &record.sketch;
    require(sk.links.is_empty() && sk.patterns.is_empty() && sk.offsets.is_empty() && sk.freeform_offsets.is_empty() && sk.wires.is_empty(), || {
        "not supported yet: sheet sketch links, patterns, offsets or 3D wires".into()
    })?;
    require(!sk.view.three_d && !sk.view.slice, || "model-only sketch view is not a sheet sketch".into())?;
    let origin = sk.points.first().ok_or_else(|| invalid("missing sketch origin"))?;
    require(origin.id == "origin" && origin.pos == Vec2::ZERO && origin.fixed && origin.link.is_none() && origin.exprs.is_none(), || {
        "sheet sketch point zero must be its fixed local origin".into()
    })?;
    let mut seen = BTreeSet::new();
    for point in &sk.points {
        identity(&mut seen, &point.id)?;
        position(point.pos)?;
        require(point.link.is_none(), || "not supported yet: linked sheet sketch point".into())?;
        if let Some(expressions) = &point.exprs {
            for expression in [&expressions.x, &expressions.y].into_iter().flatten() {
                text(expression)?;
            }
        }
    }
    for curve in &sk.curves {
        identity(&mut seen, &curve.id)?;
        require(curve.link.is_none(), || "not supported yet: linked sheet sketch curve".into())?;
        if let CurveKind::Spline { pts, degree, control, .. } = &curve.kind {
            require(
                (2..=MAX_SPLINE_POINTS).contains(&pts.len()) && (1..=5).contains(degree) && (!control || pts.len() > usize::from(*degree)),
                || "invalid sheet sketch spline size or degree".into(),
            )?;
        }
        require(curve.kind.point_ids().iter().all(|&i| i < sk.points.len()), || "sheet sketch curve references an unknown point".into())?;
        match curve.kind {
            CurveKind::Line { a, b } => require(a != b, || "sheet sketch line needs distinct point identities".into())?,
            CurveKind::Circle { r, .. } | CurveKind::Ellipse { r, .. } => {
                require(r.is_finite() && r > 0.0 && r < MAX_COORD, || "sheet sketch radius is out of range".into())?;
            }
            CurveKind::Conic { rho, .. } => require(rho.is_finite() && rho > 0.0 && rho < 1.0, || "sheet sketch conic rho is out of range".into())?,
            _ => {}
        }
        require(!curve.infinite || (curve.construction && matches!(curve.kind, CurveKind::Line { .. })), || {
            "only a construction line can be infinite in a sheet sketch".into()
        })?;
    }
    for constraint in &sk.constraints {
        identity(&mut seen, &constraint.id)?;
        sk.validate(&constraint.kind).map_err(|e| invalid(e.to_string()))?;
        if let Some(value) = &constraint.param {
            text(value)?;
        }
        if let Some(at) = constraint.text {
            position(at)?;
        }
    }
    sk.validate_periodic_splines().map_err(|e| invalid(e.to_string()))?;
    Ok(())
}

/// Sort references, not the live geometry. Sheet order stays authored; sketch
/// record order depends only on ids, so insertion order never changes saved bytes.
pub(super) fn serialize_sorted<S: Serializer>(records: &[Arc<SheetSketch>], serializer: S) -> std::result::Result<S::Ok, S::Error> {
    if records.len() > MAX_SHEET_SKETCHES {
        return Err(serde::ser::Error::custom("too many sheet sketches"));
    }
    let mut sorted: Vec<_> = records.iter().collect();
    sorted.sort_by_key(|record| record.id);
    sorted.serialize(serializer)
}

/// A sheet sketch resolved by the shared solver, in paper XY millimetres.
/// This is runtime data, not another stored sketch or a model feature. The drawing
/// and sheet ids remain explicit even when two drawings reuse the same sketch id.
#[derive(Clone, Debug)]
pub struct SolvedSheetSketch {
    pub drawing: u64,
    pub sheet: u64,
    pub id: u64,
    pub sketch: Sketch,
    pub report: solidcraft_sketch::SolveReport,
}

impl crate::Document {
    /// Resolve a stored sketch without modifying its record or the model. This
    /// public document read checks the complete drawing, including other records.
    pub fn solve_sheet_sketch(&self, drawing: u64, sketch: u64) -> Result<SolvedSheetSketch> {
        super::check_count(self.drawings.len())?;
        let mut owners = self.drawings.iter().filter(|owner| owner.id == drawing);
        let owner = owners.next().ok_or_else(|| invalid("the containing design has no such drawing"))?;
        require(owners.next().is_none(), || "sheet sketch owner id is ambiguous in this design".into())?;
        owner.validate()?;
        owner.solve_sheet_sketch(self, sketch)
    }
}

impl Drawing {
    /// Resolve one admitted record against the read-only source design. Checks
    /// owner identities, cumulative budgets and this payload, not every unrelated
    /// payload. File reads/saves and Session::edit_drawing validate the whole drawing.
    pub fn solve_sheet_sketch(&self, design: &crate::Document, sketch: u64) -> Result<SolvedSheetSketch> {
        self.validate_structure()?;
        check_budget(self.sketches.iter().map(AsRef::as_ref))?;
        let original = self.sheet_sketch(sketch).ok_or_else(|| invalid("the selected drawing has no such sheet sketch"))?;
        solve_record(design, self, original, (**original).clone())
    }

    /// Stage an atomic edit of one sketch on this drawing, without writing to the
    /// source Document. Commands call this inside Session::edit_drawing, the one
    /// publication/whole-drawing validation boundary. A library caller adopting
    /// this drawing must likewise validate it before publishing the complete record.
    ///
    /// Failed callbacks, unsafe candidates and solve failures leave self unchanged.
    /// Only the selected record is checked deeply here; unchanged Arcs are NOT a
    /// certificate that other records are valid. Their validation is never skipped
    /// by the outer publication boundary or the file reader/writer.
    pub fn edit_sheet_sketch<T>(
        &mut self,
        design: &crate::Document,
        sketch: u64,
        change: impl FnOnce(&mut Sketch) -> Result<T>,
    ) -> Result<(T, SolvedSheetSketch)> {
        self.validate_structure()?;
        let original = self.sheet_sketch(sketch).cloned().ok_or_else(|| invalid("the selected drawing has no such sheet sketch"))?;
        check_budget(self.sketches.iter().map(AsRef::as_ref))?;
        check_payload(&original)?;
        let mut candidate = (*original).clone();
        let out = change(&mut candidate.sketch)?;
        check_budget(self.sketches.iter().map(|record| if record.id == sketch { &candidate } else { record.as_ref() }))?;
        let solved = solve_record(design, self, &original, candidate)?;
        if solved.sketch != original.sketch {
            let next = SheetSketch { sketch: solved.sketch.clone(), ..(*original).clone() };
            check_budget(self.sketches.iter().map(|record| if record.id == sketch { &next } else { record.as_ref() }))?;
            let slot = self.sketches.iter_mut().find(|record| record.id == sketch).ok_or_else(|| invalid("sheet sketch disappeared"))?;
            *slot = Arc::new(next);
        }
        Ok((out, solved))
    }
}

fn solve_record(design: &crate::Document, owner: &Drawing, original: &SheetSketch, mut candidate: SheetSketch) -> Result<SolvedSheetSketch> {
    // Only the candidate is admitted before the solver sees its input-derived
    // indices. No link/pattern/offset or 3D path bypasses the existing checks.
    check_payload(&candidate)?;
    let names = parameter_names(&candidate.sketch)?;
    let mut values = if names.is_empty() {
        std::collections::BTreeMap::new()
    } else {
        require(owner.sources.len() == 1, || "sheet sketch parameters need the drawing's declared design source".into())?;
        let (values, errors) = design.param_values();
        for name in &names {
            if let Some(why) = errors.get(name) {
                return Err(invalid(format!("source parameter `{name}`: {why}")));
            }
            require(values.contains_key(name), || format!("sheet sketch: unknown source parameter `{name}`"))?;
        }
        values
    };
    // A sheet dimension owns its authored expression, not a model parameter. Keep
    // original source parameter values intact and add expression results under
    // their exact stored keys, which the shared sketch solver already consumes.
    // Mixed angle/length uses of an identical key would be ambiguous: refuse.
    let mut expression_kinds = std::collections::BTreeMap::new();
    for constraint in &candidate.sketch.constraints {
        let Some(expression) = &constraint.param else { continue };
        let is_angle = constraint.kind.is_angle();
        if let Some(previous) = expression_kinds.insert(expression.as_str(), is_angle) {
            require(previous == is_angle, || "one sheet expression cannot drive both lengths and angles".into())?;
        }
        if !values.contains_key(expression) {
            let kind = if is_angle { crate::expr::Kind::Angle } else { crate::expr::Kind::Length };
            let evaluated = crate::Document::eval_in(&values, expression, kind)?;
            let resolved = if is_angle { crate::expr::Value::angle(evaluated) } else { crate::expr::Value::length(evaluated) };
            values.insert(expression.clone(), resolved);
        }
    }
    let stored = original.sketch.driving_values();
    let report = design.solve_sketch(&values, &mut candidate.sketch, &stored, None)?;
    require(report.keeps_orientation(), || report.failure(|conflicts| format!("sheet sketch edit would conflict with {conflicts}")))?;
    check_budget(std::iter::once(&candidate))?;
    check_payload(&candidate)?;
    Ok(SolvedSheetSketch { drawing: owner.id, sheet: original.sheet, id: original.id, sketch: candidate.sketch, report })
}

/// Names are collected after record admission, with cumulative expression work
/// bounded by the existing drawing byte ceiling. Literal expressions need no
/// source and keep the expression engine's paper-mm/degree defaults.
fn parameter_names(sketch: &Sketch) -> Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    let mut bytes = 0;
    for point in &sketch.points {
        if let Some(expressions) = &point.exprs {
            for expression in [&expressions.x, &expressions.y].into_iter().flatten() {
                charge(&mut bytes, expression.len(), MAX_DRAWING_BYTES, "expression text")?;
                names.extend(crate::expr::references(expression));
            }
        }
    }
    for constraint in &sketch.constraints {
        if let Some(name) = &constraint.param {
            require(constraint.kind.is_dimension(), || "only a sheet sketch dimension can refer to a source parameter".into())?;
            charge(&mut bytes, name.len(), MAX_DRAWING_BYTES, "expression text")?;
            names.extend(crate::expr::references(name));
        }
    }
    Ok(names)
}
