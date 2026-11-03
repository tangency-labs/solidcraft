//! Commands for a view's saved automatic centre-mark rule and exclusions.

use std::sync::Arc;

use super::CommandSpec;
use super::drawing_dimension::status_words;
use crate::params::bad;
use crate::schema::drawing_auto_mark::{ExcludeParams, ExclusionResult, IncludeParams, RuleResult, SetParams, ViewParams};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};
use serde_json::Value;
use solidcraft_doc::drawing::auto_marks::resolve_exclusions;
use solidcraft_doc::drawing::dimension::ModelEdge;
use solidcraft_doc::drawing::view::AutoMarks;
use solidcraft_doc::ids;

const SET: &str = "drawing.auto_center_mark.set";
const INSPECT: &str = "drawing.auto_center_mark.inspect";
const EXCLUDE: &str = "drawing.auto_center_mark.exclude";
const INCLUDE: &str = "drawing.auto_center_mark.include";
const SET_CONTRACT: TypedContract<SetParams> = TypedContract::new();
const VIEW_CONTRACT: TypedContract<ViewParams> = TypedContract::new();
const EXCLUDE_CONTRACT: TypedContract<ExcludeParams> = TypedContract::new();
const INCLUDE_CONTRACT: TypedContract<IncludeParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new(SET, "Set Automatic Center Marks", crate::checked!("drawing.auto_center_mark.set", set))
        .icon("edit")
        .in_drawing()
        .doc_only()
        .contract(&SET_CONTRACT)
        .output(output_schema_for::<RuleResult>)
        .params("view: id; enabled: bool. Disabling removes the rule and its exclusions; one undo step."),
    CommandSpec::new(INSPECT, "Inspect Automatic Center Marks", crate::checked!("drawing.auto_center_mark.inspect", inspect))
        .icon("inspect")
        .in_drawing()
        .noundo()
        .doc_only()
        .contract(&VIEW_CONTRACT)
        .output(output_schema_for::<RuleResult>)
        .params("view: id → saved exclusions and their current reference statuses."),
    CommandSpec::new(EXCLUDE, "Exclude Automatic Center Mark", crate::checked!("drawing.auto_center_mark.exclude", exclude))
        .icon("delete")
        .in_drawing()
        .doc_only()
        .contract(&EXCLUDE_CONTRACT)
        .output(output_schema_for::<RuleResult>)
        .params("view: id; edge: model edge pick with optional occurrence path. One undo step."),
    CommandSpec::new(INCLUDE, "Include Automatic Center Mark", crate::checked!("drawing.auto_center_mark.include", include))
        .icon("new")
        .in_drawing()
        .doc_only()
        .contract(&INCLUDE_CONTRACT)
        .output(output_schema_for::<RuleResult>)
        .params("view: id; reference: saved exclusion as returned by inspect, even when lost. One undo step."),
];

fn set(s: &mut Session, p: &Value) -> Result<Value> {
    let request: SetParams = crate::schema::read(SET, p)?;
    let id = request.view.0;
    let changed = s.edit_drawing(SET, |d| {
        let i = d.view_index(id).ok_or_else(|| bad(SET, "the drawing has no such view"))?;
        let view = d.views.get_mut(i).map(Arc::make_mut).ok_or_else(|| bad(SET, "the view no longer exists"))?;
        if view.centre_marks.is_some() == request.enabled {
            return Ok(false);
        }
        view.centre_marks = request.enabled.then(AutoMarks::default);
        Ok(true)
    })?;
    answer(s, SET, id, changed)
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let request: ViewParams = crate::schema::read(INSPECT, p)?;
    answer(s, INSPECT, request.view.0, false)
}

fn exclude(s: &mut Session, p: &Value) -> Result<Value> {
    let request: ExcludeParams = crate::schema::read(EXCLUDE, p)?;
    let id = request.view.0;
    let view = s.drawing(EXCLUDE)?.view(id).ok_or_else(|| bad(EXCLUDE, "the drawing has no such view"))?;
    if view.centre_marks.is_none() {
        return Err(bad(EXCLUDE, "automatic centre marks are disabled on this view"));
    }
    let edge = s.doc.edge_reference(&s.model, &request.edge.pick, &request.edge.path).map_err(|e| bad(EXCLUDE, e.to_string()))?;
    let candidates = crate::drawing_scene::automatic_mark_candidates(s, id).map_err(|e| bad(EXCLUDE, e.to_string()))?;
    if !candidates.iter().any(|candidate| candidate.sources.contains(&edge)) {
        return Err(bad(EXCLUDE, "the edge is not part of an eligible automatic centre mark"));
    }
    let changed = s.edit_drawing(EXCLUDE, |d| {
        let i = d.view_index(id).ok_or_else(|| bad(EXCLUDE, "the drawing has no such view"))?;
        let view = d.views.get_mut(i).map(Arc::make_mut).ok_or_else(|| bad(EXCLUDE, "the view no longer exists"))?;
        let rule = view.centre_marks.as_mut().ok_or_else(|| bad(EXCLUDE, "automatic centre marks are disabled on this view"))?;
        if rule.exclude.contains(&edge) {
            return Ok(false);
        }
        rule.exclude.push(edge);
        Ok(true)
    })?;
    answer(s, EXCLUDE, id, changed)
}

fn include(s: &mut Session, p: &Value) -> Result<Value> {
    let request: IncludeParams = crate::schema::read(INCLUDE, p)?;
    let id = request.view.0;
    let reference: ModelEdge = serde_json::from_value(request.reference).map_err(|e| bad(INCLUDE, e.to_string()))?;
    let changed = s.edit_drawing(INCLUDE, |d| {
        let i = d.view_index(id).ok_or_else(|| bad(INCLUDE, "the drawing has no such view"))?;
        let view = d.views.get_mut(i).map(Arc::make_mut).ok_or_else(|| bad(INCLUDE, "the view no longer exists"))?;
        let rule = view.centre_marks.as_mut().ok_or_else(|| bad(INCLUDE, "automatic centre marks are disabled on this view"))?;
        let Some(index) = rule.exclude.iter().position(|item| *item == reference) else { return Ok(false) };
        rule.exclude.remove(index);
        Ok(true)
    })?;
    answer(s, INCLUDE, id, changed)
}

fn answer(s: &Session, cmd: &str, id: u64, changed: bool) -> Result<Value> {
    let d = s.drawing(cmd)?;
    let view = d.view(id).ok_or_else(|| bad(cmd, "the drawing has no such view"))?;
    let mut exclusions = Vec::new();
    if let Some(rule) = &view.centre_marks {
        let resolved = resolve_exclusions(&s.doc, &s.model, &rule.exclude).map_err(|e| bad(cmd, e.to_string()))?;
        for (edge, r) in rule.exclude.iter().zip(resolved) {
            let (status, why, candidates) = status_words(r.status);
            exclusions.push(ExclusionResult {
                reference: serde_json::to_value(edge).map_err(|e| EngineError::Other(e.to_string()))?,
                status: status.into(),
                why,
                exact: r.exact,
                candidates,
            });
        }
    }
    serde_json::to_value(RuleResult { view: ids::encode(id), enabled: view.centre_marks.is_some(), exclusions, changed, param_warnings: Vec::new() })
        .map_err(|e| EngineError::Other(e.to_string()))
}
