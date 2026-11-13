//! `drawing.check` (#13054): what makes the open drawing wrong or incomplete, as data that a person
//! or a script acts on. Each finding comes from the rule that already says it: each sheet as an
//! export reads it ([`crate::drawing_scene::as_shown`]), each view's place there ([`crate::drawing_view::Room`]),
//! the balloons' and notes' references as the model has them now, the title blocks' text as the
//! sheet shows it ([`title_block_text::shown`]), and each parts list's balloon counts as
//! `drawing.inspect` gives them. Beside the findings, each feature's coverage: whether a dimension
//! or note points at a face it made, by the saved names ([`Timeline::covers`]), and whether a view
//! draws one. Nothing is changed.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::Value;
use solidcore_record::Vec2;
use solidcraft_doc::bom::Tree;
use solidcraft_doc::drawing::Drawing;
use solidcraft_doc::drawing::coverage;
use solidcraft_doc::drawing::dimension::Status;
use solidcraft_doc::{ModelState, ids};
use solidcraft_geom::paper::{Item, ObjectKind};
use solidcraft_geom::{Aabb2, EPS};

use super::CommandSpec;
use crate::drawing_scene::{SheetScene, Status as ViewStatus, ViewState};
use crate::drawing_view::Room;
use crate::params::bad;
use crate::schema::drawing_check::{
    CheckParams, CheckResult, CheckedKind, CheckedObject, Coverage, CoverageState, Finding, FindingKind, KindCount, StateCount,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::title_block_text;
use crate::{EngineError, Result, Session};

const CHECK: &str = "drawing.check";
const CONTRACT: TypedContract<CheckParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(CHECK, "Check Drawing", crate::checked!("drawing.check", check))
    .icon("inspect")
    .in_drawing()
    .noundo()
    .doc_only()
    .contract(&CONTRACT)
    .output(output_schema_for::<CheckResult>)
    .params("sheet?: id or name (default: every sheet) → {sheet?, findings: [{kind, sheet, object: {kind: view|dimension|centre_line|centre_mark|balloon|note|table|image|title_block, id}, cell?, field?, item?, why}], counts: [{kind, count}], coverage: [{feature, name, state, by?: [{kind: dimension|note, id}], why?}], coverage_counts: [{state, count}]}: what makes the drawing wrong or incomplete, in sheet order, then by the record each names. kind is one of a closed list: lost, ambiguous, unmeasurable, suppressed, failed (a dimension, balloon or note whose reference isn't found as one thing, can't be measured, or depends on a suppressed or failed feature; failed is also a view or centre annotation that can't be drawn; an automatic centre mark failure names its view), overridden (a dimension whose text is typed over its value, or a title block cell whose typed text hides a value), out_of_date (a view or dimension a deferred drawing shows as it last was), nothing_shown (a view with nothing to measure), empty_field (a title block field with no value, as source.key; a revision field only once the drawing has a revision row; a cell whose typed text fills it is none), not_ballooned (a row of a parts list on the sheet that no balloon points at, by item), outside_sheet (a view outside its sheet's drawing space or over its title block; a dimension, centre annotation, image or table off the paper). coverage lists each feature of the design in timeline order, apart from the findings: covered (a dimension or note points at a face it made: an edge covers the latest feature among the faces it separates, a corner the latest among its edges' faces, a pattern's copy the pattern; by lists them), not_covered (a view draws a face it made, named from exact history, and nothing points at it), unknown (the faces a view draws of it aren't named from exact history), not_applicable (it is suppressed or rolled back, only reference parts draw it, or no view draws a face it made: a sketch, a plane). Coverage says a dimension or a note points at a feature, not that it is fully dimensioned. With sheet, the findings and the coverage are of that sheet only, and sheet echoes its id. The sheet is read as an export reads it: a deferred drawing is checked as it shows the model, with what it shows as it last was listed. Read-only: nothing is drawn or saved, and the same drawing always gives the same answer.")];

fn check(s: &mut Session, p: &Value) -> Result<Value> {
    let request: CheckParams = crate::schema::read(CHECK, p)?;
    let d = s.drawing(CHECK)?.clone();
    let sheets: Vec<u64> = match &request.sheet {
        Some(named) => vec![crate::drawing_scene::sheet_id(&d, named, CHECK)?],
        None => d.sheets.iter().map(|sheet| sheet.id).collect(),
    };
    // Each sheet read once, as it shows the model: for its findings and for what its views draw.
    let mut findings = Vec::new();
    let (mut drawn, mut seen) = (coverage::Drawn::default(), Vec::new());
    for &sheet in &sheets {
        let shown = crate::drawing_scene::as_shown(s, &d, sheet)?;
        add_drawn(&mut drawn, &mut seen, &shown);
        findings.extend(on_sheet(&d, sheet, shown)?);
    }
    findings.extend(references(s, &d, &sheets)?);
    findings.extend(fields_and_rows(s, &d, &sheets));
    // In sheet order, then by the record each names: ids decode, as every finding's came from one.
    let at = |id: &str| ids::decode(id).and_then(|id| d.sheets.iter().position(|sheet| sheet.id == id));
    let key = |f: &Finding| (at(&f.sheet), ids::decode(&f.object.id), f.kind, f.cell.clone(), f.field.clone(), f.item.clone());
    findings.sort_by_cached_key(key);
    let counts = FindingKind::ALL
        .iter()
        .copied()
        .map(|kind| KindCount { kind, count: findings.iter().filter(|f| f.kind == kind).count() })
        .filter(|count| count.count > 0)
        .collect();
    let coverage = feature_coverage(s, &d, &sheets, &drawn);
    let coverage_counts = CoverageState::ALL
        .into_iter()
        .map(|state| StateCount { state, count: coverage.iter().filter(|c| c.state == state).count() })
        .filter(|count| count.count > 0)
        .collect();
    let sheet = request.sheet.as_ref().and(sheets.first()).map(|&id| ids::encode(id));
    let result = CheckResult { sheet, findings, counts, coverage, coverage_counts, param_warnings: Vec::new() };
    serde_json::to_value(result).map_err(|e| EngineError::Other(e.to_string()))
}

fn finding(kind: FindingKind, sheet: u64, object: CheckedObject, why: String) -> Finding {
    Finding { kind, sheet: ids::encode(sheet), object, cell: None, field: None, item: None, why }
}

fn object(kind: CheckedKind, id: u64) -> CheckedObject {
    CheckedObject { kind, id: ids::encode(id) }
}

/// What sheet `sheet` shows (`shown`): its views that draw nothing, its dimensions that don't stand
/// with the model, what it shows as it last was, and what lies outside it.
fn on_sheet(d: &Drawing, sheet: u64, shown: SheetScene) -> Result<Vec<Finding>> {
    let mut out: Vec<Finding> = shown.views.iter().filter_map(|view| view_finding(sheet, view)).collect();
    let dimensions =
        shown.dimensions.into_iter().filter_map(|state| status_finding(sheet, object(CheckedKind::Dimension, state.dimension), state.status));
    out.extend(dimensions);
    // These failures belong to the accepted sheet, including a deferred sheet's held frame.
    // Automatic marks belong to the view; manual annotations name their own records.
    out.extend(shown.annotation_failures.into_iter().filter_map(|failure| {
        let kind = match failure.kind {
            ObjectKind::CentreLine => CheckedKind::CentreLine,
            ObjectKind::CentreMark if failure.object == failure.view => CheckedKind::View,
            ObjectKind::CentreMark => CheckedKind::CentreMark,
            _ => return None,
        };
        Some(finding(FindingKind::Failed, sheet, object(kind, failure.object), failure.reason))
    }));
    // Out of date as a deferred drawing shows it as it last was, or held back by a view that draws its
    // parts apart, which no update draws.
    let held = crate::drawing_scene::held_apart(d, d.views.iter().filter(|v| v.sheet == sheet).map(|v| v.id));
    let last = "the sheet shows it as it last was: drawing.update draws it from the model as it is now";
    let view_of = |id: u64| d.dimension(id).map(|r| r.view).or_else(|| d.notes.iter().find(|n| n.id == id)?.anchor.view());
    let apart = |id: u64| view_of(id).and_then(|view| crate::drawing_view::draws_apart(d, view));
    for id in shown.out_of_date {
        let kind = if d.view(id).is_some() {
            CheckedKind::View
        } else if d.dimension(id).is_some() {
            CheckedKind::Dimension
        } else {
            CheckedKind::Note
        };
        let why = apart(id).filter(|_| held.contains(&id)).map_or(last, crate::drawing_view::Apart::held);
        out.push(finding(FindingKind::OutOfDate, sheet, object(kind, id), why.into()));
    }
    // A view belongs where its sheet's Room says, by its boundary as the sheet shows it. Anything
    // else is held to the paper, whose box is display data, as the scene's boxes are: it decides
    // nothing in the model.
    let record = crate::drawing_view::sheet(d, sheet, CHECK)?;
    let room = Room::on(record, CHECK)?;
    let size = record.sheet.size_mm().map_err(|e| bad(CHECK, e.to_string()))?;
    let paper = Aabb2 { min: Vec2::ZERO, max: size }.inflated(EPS);
    let off = |bounds: Aabb2| !(paper.contains(bounds.min) && paper.contains(bounds.max));
    let off_paper = |kind, id| finding(FindingKind::OutsideSheet, sheet, object(kind, id), "it lies off the paper".into());
    for placed in &shown.objects {
        let kind = match placed.kind {
            ObjectKind::View => {
                if placed.boundary_mm.is_some_and(|boundary| !room.holds(&boundary)) {
                    let why = "it lies outside its sheet's drawing space or over its title block".into();
                    out.push(finding(FindingKind::OutsideSheet, sheet, object(CheckedKind::View, placed.object), why));
                }
                continue;
            }
            ObjectKind::Dimension => CheckedKind::Dimension,
            ObjectKind::CentreLine => CheckedKind::CentreLine,
            ObjectKind::CentreMark => CheckedKind::CentreMark,
            ObjectKind::Note => CheckedKind::Note,
            ObjectKind::PartsList | ObjectKind::RevisionTable => CheckedKind::Table,
            // Images are painted with the border and aren't among the objects: read below.
            ObjectKind::SheetImage | ObjectKind::SheetBorder | ObjectKind::DrawingBorder | ObjectKind::TitleBlock => continue,
        };
        if off(placed.bounds_mm) {
            out.push(off_paper(kind, placed.object));
        }
    }
    // Automatic marks are not selectable objects. Check their accepted scene groups directly,
    // naming their owning view once even if several marks cross the paper edge.
    let views: HashSet<_> = shown.views.iter().map(|view| view.view).collect();
    let mut off_marks = HashSet::new();
    for group in &shown.scene.groups {
        if group.kind == ObjectKind::CentreMark && views.contains(&group.object) {
            let bounds = crate::drawing_scene::group_bounds(group, &shown.scene.fonts);
            if !bounds.is_empty() && off(bounds) && off_marks.insert(group.object) {
                out.push(finding(
                    FindingKind::OutsideSheet,
                    sheet,
                    object(CheckedKind::View, group.object),
                    "its automatic centre marks lie off the paper".into(),
                ));
            }
        }
    }
    // Each image's box is the picture the sheet paints for it.
    for group in crate::drawing_pictures::groups(d, sheet) {
        if group.items.iter().any(|item| matches!(item, Item::Picture(picture) if off(picture.rect))) {
            out.push(off_paper(CheckedKind::Image, group.object));
        }
    }
    Ok(out)
}

/// A view's finding: one that draws nothing, with why.
fn view_finding(sheet: u64, view: &ViewState) -> Option<Finding> {
    let kind = match view.status {
        ViewStatus::Drawn => return None,
        ViewStatus::Placeholder(_) => FindingKind::NothingShown,
        ViewStatus::Failed(_) => FindingKind::Failed,
    };
    Some(finding(kind, sheet, object(CheckedKind::View, view.view), view.status.reason().unwrap_or_default()))
}

/// A reference's finding, by where it stands with the model: none when it is found and measured.
fn status_finding(sheet: u64, object: CheckedObject, status: Status) -> Option<Finding> {
    let (kind, why) = match status {
        Status::Ok => return None,
        Status::Overridden => (FindingKind::Overridden, "its text is typed over its measured value".into()),
        Status::Ambiguous(_) => (FindingKind::Ambiguous, "its reference finds more than one thing, and none is chosen".into()),
        Status::Suppressed => (FindingKind::Suppressed, "a feature it depends on is suppressed or rolled back".into()),
        Status::Failed(why) => (FindingKind::Failed, why),
        Status::Lost(why, _) => (FindingKind::Lost, why),
        Status::Unmeasurable(why) => (FindingKind::Unmeasurable, why),
    };
    Some(finding(kind, sheet, object, why))
}

/// The balloons and notes in views on `sheets` whose references don't stand with the model: one
/// read of each kind for the drawing, each body's names worked out once. A note on a sheet reads
/// no model, so it is never read here.
fn references(s: &Session, d: &Drawing, sheets: &[u64]) -> Result<Vec<Finding>> {
    let on = |view: u64| d.view(view).map(|v| v.sheet).filter(|sheet| sheets.contains(sheet));
    let balloons: Vec<(u64, u64)> = d.balloons.iter().filter_map(|b| Some((b.id, on(b.view)?))).collect();
    let notes: Vec<(u64, u64)> = d.notes.iter().filter_map(|n| Some((n.id, on(n.anchor.view()?)?))).collect();
    let wanted = |records: &[(u64, u64)]| records.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    let read_balloons = s.doc.resolve_balloons(&s.model, d, &wanted(&balloons)).map_err(|e| bad(CHECK, e.to_string()))?;
    let read_notes = s.doc.resolve_notes(&s.model, d, &wanted(&notes)).map_err(|e| bad(CHECK, e.to_string()))?;
    let balloons = balloons.iter().zip(read_balloons).map(|((id, sheet), r)| (*sheet, object(CheckedKind::Balloon, *id), r.status));
    let mut out: Vec<Finding> = balloons.filter_map(|(sheet, object, status)| status_finding(sheet, object, status)).collect();
    // A note's anchor stands as a reference does; each branch that doesn't is a finding of its own, by
    // its number, as a lost branch loses only itself.
    for ((id, sheet), r) in notes.iter().zip(read_notes) {
        out.extend(status_finding(*sheet, object(CheckedKind::Note, *id), r.anchor.status));
        for (n, end) in (1..).zip(r.branches) {
            let branch = status_finding(*sheet, object(CheckedKind::Note, *id), end.status);
            out.extend(branch.map(|f| Finding { why: format!("branch {n}: {}", f.why), ..f }));
        }
    }
    Ok(out)
}

/// The title block fields on `sheets` with no value, the cells whose typed text hides one, and the
/// rows of their parts lists that no balloon points at: one tree for the drawing, so each
/// component is weighed and listed once.
fn fields_and_rows(s: &Session, d: &Drawing, sheets: &[u64]) -> Vec<Finding> {
    let state = s.model.state();
    let mut tree = Tree::new(&s.doc, &state);
    let mut out = Vec::new();
    for (at, sheet) in d.sheets.iter().enumerate().filter(|(_, sheet)| sheets.contains(&sheet.id)) {
        title_block_text::shown_with(&s.doc, &mut tree, d, at, |cell, content, values| {
            let block = object(CheckedKind::TitleBlock, sheet.id);
            match cell.replaced {
                // Typed text over an empty value fills the cell: nothing is hidden.
                Some(hidden) if !hidden.is_empty() => {
                    let why = format!("its typed text \"{}\" hides its value \"{hidden}\"", cell.text);
                    out.push(Finding { cell: Some(cell.name), ..finding(FindingKind::Overridden, sheet.id, block, why) });
                }
                Some(_) => {}
                // Read only here: the sheet itself never needs them.
                None => out.extend(values.empty_fields(content).into_iter().map(|field| {
                    let why = format!("{field} has no value");
                    Finding { cell: Some(cell.name.clone()), field: Some(field), ..finding(FindingKind::EmptyField, sheet.id, block.clone(), why) }
                })),
            }
        });
    }
    // A parts list that can't be made has no rows to count.
    for (table, rows) in super::drawing::ballooned(d, &mut tree) {
        let Some(sheet) = d.tables.by_id(table).map(|t| t.sheet).filter(|sheet| sheets.contains(sheet)) else { continue };
        for (item, _) in rows.into_iter().flatten().filter(|(_, balloons)| *balloons == 0) {
            let why = format!("no balloon points at row {item}");
            out.push(Finding { item: Some(item), ..finding(FindingKind::NotBallooned, sheet, object(CheckedKind::Table, table), why) });
        }
    }
    out
}

/// Each feature of the design, in timeline order, and whether the dimensions and notes on `sheets`
/// point at a face it made ([`coverage::of`]), with what the views on them draw (`drawn`). One that a
/// view drawing its parts apart holds back covers nothing: it isn't on paper, and the findings name it
/// out of date.
fn feature_coverage(s: &Session, d: &Drawing, sheets: &[u64], drawn: &coverage::Drawn) -> Vec<Coverage> {
    let on = |view: u64| d.view(view).is_some_and(|v| sheets.contains(&v.sheet));
    let held = crate::drawing_scene::held_apart(d, d.views.iter().filter(|v| sheets.contains(&v.sheet)).map(|v| v.id));
    let dimensions = d.dimensions.iter().filter(|dim| on(dim.view) && !held.contains(&dim.id));
    let dimensions = dimensions.map(|dim| (object(CheckedKind::Dimension, dim.id), dim.kind.references()));
    let notes = d.notes.iter().filter(|n| n.anchor.view().is_some_and(on) && !held.contains(&n.id));
    let notes = notes.map(|n| (object(CheckedKind::Note, n.id), n.references().collect()));
    let state = |state: coverage::State| match state {
        coverage::State::Covered => CoverageState::Covered,
        coverage::State::NotCovered => CoverageState::NotCovered,
        coverage::State::Unknown => CoverageState::Unknown,
        coverage::State::NotApplicable => CoverageState::NotApplicable,
    };
    let features = coverage::of(&s.doc, dimensions.chain(notes), drawn).into_iter();
    features.map(|f| Coverage { feature: f.feature, name: f.name, state: state(f.state), by: f.by, why: f.why.map(str::to_string) }).collect()
}

/// What the views in `shown` draw, as the sheet shows it, each body once however many views and
/// sheets draw it ([`first_read`]). A view that draws no lines draws nothing here; its finding says
/// why.
fn add_drawn(drawn: &mut coverage::Drawn, seen: &mut Vec<(Arc<ModelState>, HashSet<usize>)>, shown: &SheetScene) {
    for (state, shown) in shown.bodies_drawn() {
        if first_read(seen, state, shown.index)
            && let Some(body) = state.bodies.get(shown.index)
        {
            drawn.body(body, shown.reference);
        }
    }
}

/// Whether body `index` of `state` is read for the first time, by the bodies `seen` read of each
/// world state. A state is told by its identity, so each is held here: none freed while it is
/// compared can lend its address to another. What is read never depends on this, only how often.
fn first_read(seen: &mut Vec<(Arc<ModelState>, HashSet<usize>)>, state: &Arc<ModelState>, index: usize) -> bool {
    let at = seen.iter().position(|(held, _)| Arc::ptr_eq(held, state)).unwrap_or_else(|| {
        seen.push((state.clone(), HashSet::new()));
        seen.len() - 1
    });
    seen.get_mut(at).is_some_and(|(_, read)| read.insert(index))
}

#[cfg(test)]
#[path = "drawing_check_tests.rs"]
mod tests;
