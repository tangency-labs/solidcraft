//! Explicit edit transitions shared by commands and UI tool entry. Runtime events are not history.
use crate::{CommandSpec, EngineError, Result, Session};
use serde_json::{Value, json};
use std::sync::Arc;

/// Which sketch receiver finished, independent of the currently focused workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SketchDomain {
    Model,
    Sheet { drawing: u64, sheet: u64 },
}

/// A committed edit exit. Hosts consume events once, including after an incoming command fails.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SketchFinishEvent {
    pub sequence: u64,
    pub sketch: u64,
    pub name: String,
    pub view: Option<u64>,
    pub domain: SketchDomain,
}

/// Reviewed per-command semantics; command authority and toolbar placement are unrelated.
#[derive(Clone, Copy, Default)]
pub enum EditStart {
    #[default]
    Preserve,
    Begin,
    Conditional(fn(&Value) -> bool),
    SelectionDelete,
}
impl EditStart {
    pub fn starts(self, params: &Value) -> bool {
        match self {
            Self::Preserve => false,
            Self::Begin => true,
            Self::Conditional(predicate) => predicate(params),
            Self::SelectionDelete => false,
        }
    }
}

/// Mixed annotation/joint setters preserve calls that only rename, hide or show their receiver.
pub(crate) fn changes_editor_fields(params: &Value) -> bool {
    let fields = params.get("set").unwrap_or(params);
    let Some(fields) = fields.as_object() else { return true };
    if fields.is_empty() {
        return true;
    }
    fields.keys().any(|key| {
        !matches!(
            key.as_str(),
            "id" | "note"
                | "joint"
                | "table"
                | "view"
                | "fill"
                | "tag"
                | "drawing"
                | "sheet"
                | "name"
                | "hidden"
                | "visible"
                | "hide"
                | "show"
                | "shown"
        )
    })
}

pub(crate) fn finished_reply(mut value: Value, event: Option<&SketchFinishEvent>) -> Value {
    if let Some(event) = event
        && let Some(object) = value.as_object_mut()
    {
        object.insert("finished_sketch".into(), json!(event.name));
    }
    value
}

impl Session {
    /// Framing a canvas adds geometry to the current Model sketch. Sheet sketches cannot
    /// receive a Model canvas frame. A form without an image has not chosen its operation yet.
    fn canvas_continues_model_sketch(&self, command: &str, params: &Value) -> bool {
        if self.active_sketch.is_none() || self.active_sheet_sketch.is_some() {
            return false;
        }
        match command {
            "canvas.frame_sketch" => true,
            "canvas.insert" | "canvas.decal" => {
                let has_image = ["path", "data"].iter().any(|key| params.get(*key).is_some_and(|value| !value.is_null()));
                // Invalid frame values remain inside the existing edit until input validation
                // rejects them; they must not commit the sketch as a side effect of refusal.
                !has_image || !matches!(params.get("frame"), None | Some(Value::Null) | Some(Value::Bool(false)))
            }
            _ => false,
        }
    }

    fn edit_receiver_is_focused(&self) -> bool {
        if let Some(entry) = &self.active_sheet_sketch {
            !self.model_focus && self.open_drawing == Some(entry.drawing)
        } else {
            (self.model_focus || self.open_drawing.is_none()) && self.sketching_view.is_none_or(|view| view == self.view_context.focused())
        }
    }

    /// Lifecycle commands address the one active edit while preserving the host's receiver.
    pub(crate) fn with_sketch_exit_receiver<T>(&mut self, command: &str, run: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        if !matches!(command, "sketch.finish" | "sketch.cancel") || (self.active_sketch.is_none() && self.active_sheet_sketch.is_none()) {
            return run(self);
        }
        let previous = (self.open_drawing, self.model_focus);
        let selection = (!self.edit_receiver_is_focused()).then(|| self.selection.clone());
        if let Some(entry) = &self.active_sheet_sketch {
            self.open_drawing = Some(entry.drawing);
            self.model_focus = false;
        } else {
            self.model_focus = true;
        }
        let result = run(self);
        (self.open_drawing, self.model_focus) = previous;
        if let Some(selection) = selection {
            self.selection = selection;
        }
        result
    }

    /// Semantic metadata for a command, including the selected receiver of Delete.
    pub fn command_starts_edit(&self, spec: &CommandSpec, params: &Value) -> bool {
        if self.canvas_continues_model_sketch(spec.id, params) {
            return false;
        }
        if matches!(spec.edit_start, EditStart::SelectionDelete) {
            if params.get("dry_run").and_then(Value::as_bool) == Some(true) {
                return false;
            }
            if ["components", "occurrences", "canvases", "point_clouds", "joints"]
                .iter()
                .any(|key| params.get(*key).and_then(Value::as_array).is_some_and(|items| !items.is_empty()))
            {
                return true;
            }
            let selected = params
                .get("items")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(|item| serde_json::from_value::<crate::Sel>(item.clone()).ok()).collect::<Vec<_>>());
            return selected
                .as_deref()
                .unwrap_or(&self.selection)
                .iter()
                .any(|item| matches!(item, crate::Sel::Body { .. } | crate::Sel::Feature { .. } | crate::Sel::Occurrence { .. }));
        }
        spec.starts_edit(params)
    }

    /// Finish the current edit before entering a registered tool. Does not evaluate geometry;
    /// hosts retain their existing deferred preparation path when evaluation is pending.
    pub fn prepare_edit_start(&mut self, command: &str, params: &Value) -> Result<Option<SketchFinishEvent>> {
        let spec = crate::find_command(command).ok_or_else(|| EngineError::UnknownCommand(command.into()))?;
        self.check_command(spec, params)?;
        if self.command_starts_edit(spec, params) && self.prepared_edit_intent.as_deref() != Some(spec.id) {
            self.edit_start_sequence = self.edit_start_sequence.saturating_add(1);
            self.prepared_edit_intent = Some(spec.id.into());
        }
        let event = self.prepare_command_edit(spec, params)?;
        if self.command_starts_edit(spec, params) {
            if let Some(event) = event.as_ref().filter(|event| event.domain == SketchDomain::Model) {
                self.prepared_edit_source = Some((spec.id.into(), event.clone()));
            } else if self.prepared_edit_source.as_ref().is_some_and(|(command, _)| command != spec.id) {
                self.prepared_edit_source = None;
            }
        }
        Ok(event)
    }

    /// Discard a tool's unused source context when its editor is cancelled.
    pub fn cancel_prepared_edit_start(&mut self) {
        self.prepared_edit_source = None;
        self.prepared_edit_intent = None;
    }

    /// Changes when a semantic editor starts; successful worker adoption merges only new starts.
    pub fn edit_start_sequence(&self) -> u64 {
        self.edit_start_sequence
    }

    pub(crate) fn note_edit_entry(&mut self, spec: &CommandSpec, params: &Value) {
        if self.edit_entry_depth == 0 && self.command_starts_edit(spec, params) && self.prepared_edit_intent.take().as_deref() != Some(spec.id) {
            self.edit_start_sequence = self.edit_start_sequence.saturating_add(1);
        }
    }

    pub(crate) fn take_prepared_edit_source(&mut self, spec: &CommandSpec, params: &Value) -> Option<SketchFinishEvent> {
        if self.edit_entry_depth != 0 || !self.command_starts_edit(spec, params) {
            return None;
        }
        self.prepared_edit_source.take().filter(|(command, _)| command == spec.id).map(|(_, event)| event)
    }

    /// Enter the earlier-feature editor, independently of dimensional timeline.edit updates.
    pub fn prepare_feature_edit_start(&mut self, feature: u64) -> Result<Option<SketchFinishEvent>> {
        if self.doc.feature(feature).is_none() {
            return Err(EngineError::Other(format!("no feature {feature}")));
        }
        let spec = crate::find_command("timeline.redefine").ok_or_else(|| EngineError::UnknownCommand("timeline.redefine".into()))?;
        self.check_command(spec, &json!({"feature":feature}))?;
        self.cancel_prepared_edit_start();
        self.edit_start_sequence = self.edit_start_sequence.saturating_add(1);
        self.finish_open_edits()
    }

    /// Closing another view preserves the edit; closing its owner commits the same exit event.
    pub fn prepare_view_close(&mut self, view: u64) -> Result<Option<SketchFinishEvent>> {
        let spec = crate::find_command("tab.view.close").ok_or_else(|| EngineError::UnknownCommand("tab.view.close".into()))?;
        self.check_command(spec, &json!({"id":view}))?;
        if self.sketching_view != Some(view) {
            return Ok(None);
        }
        self.cancel_prepared_edit_start();
        self.edit_start_sequence = self.edit_start_sequence.saturating_add(1);
        self.finish_open_edits()
    }

    /// Whether the host should prepare the restored full model off its window thread.
    pub fn needs_sketch_finish_evaluation(&self) -> bool {
        self.sketch_finish_pending
    }

    /// Consume one newly committed exit, exactly once. History never restores this queue.
    pub fn take_sketch_finish_event(&mut self) -> Option<SketchFinishEvent> {
        self.sketch_finish_events.pop_front()
    }

    pub(crate) fn publish_sketch_finish(&mut self, mut event: SketchFinishEvent) {
        self.sketch_finish_sequence = self.sketch_finish_sequence.saturating_add(1);
        event.sequence = self.sketch_finish_sequence;
        self.sketch_finish_events.push_back(event);
        if self.sketch_finish_events.len() > crate::MAX_UNDO {
            self.sketch_finish_events.pop_front();
        }
    }

    pub(crate) fn prepare_command_edit(&mut self, spec: &CommandSpec, params: &Value) -> Result<Option<SketchFinishEvent>> {
        if self.edit_entry_depth != 0 || !self.command_starts_edit(spec, params) {
            return Ok(None);
        }
        if spec.id == "sketch.edit"
            && let Some(key) = params.get("sketch").and_then(|value| match value {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            && self.doc.find_feature(&key).is_some_and(|feature| Some(feature.id) == self.active_sketch)
        {
            return Ok(None);
        }
        if spec.id == "drawing.sketch.edit"
            && let Some(entry) = &self.active_sheet_sketch
            && params.get("sketch").is_some_and(|value| {
                value.as_u64() == Some(entry.sketch) || value.as_str().is_some_and(|id| solidcraft_doc::ids::decode(id) == Some(entry.sketch))
            })
        {
            return Ok(None);
        }
        self.finish_open_edits()
    }

    fn finish_open_edits(&mut self) -> Result<Option<SketchFinishEvent>> {
        let selection = ((self.active_sketch.is_some() || self.active_sheet_sketch.is_some()) && !self.edit_receiver_is_focused())
            .then(|| self.selection.clone());
        let result = self.finish_edit_receivers();
        if let Some(selection) = selection {
            self.selection = selection;
        }
        result
    }

    fn finish_edit_receivers(&mut self) -> Result<Option<SketchFinishEvent>> {
        let mut first = None;
        if let Some(sketch) = self.active_sketch {
            let mut event = SketchFinishEvent {
                sequence: 0,
                sketch,
                name: self.doc.feature(sketch).map(|feature| feature.name.clone()).unwrap_or_default(),
                view: self.sketching_view,
                domain: SketchDomain::Model,
            };
            let before = self.doc.clone();
            crate::cmd::finish_model_edit(self, true)?;
            self.sketch_finish_pending |= !Arc::ptr_eq(&before, &self.doc) || self.model.is_stale();
            self.publish_sketch_finish(event.clone());
            event.sequence = self.sketch_finish_sequence;
            first = Some(event);
        }
        if let Some(entry) = self.active_sheet_sketch.clone() {
            let (name, empty) = self
                .doc
                .drawing(entry.drawing)
                .and_then(|drawing| drawing.sheet_sketch(entry.sketch))
                .map(|record| (record.name.clone(), empty_geometry(&record.sketch)))
                .unwrap_or_default();
            let mut event = SketchFinishEvent {
                sequence: 0,
                sketch: entry.sketch,
                name,
                view: None,
                domain: SketchDomain::Sheet { drawing: entry.drawing, sheet: entry.sheet },
            };
            if empty && let Some(drawing) = self.doc_mut().drawings.iter_mut().find(|drawing| drawing.id == entry.drawing) {
                Arc::make_mut(drawing).sketches.retain(|record| record.id != entry.sketch);
            }
            self.active_sheet_sketch = None;
            self.selection.clear();
            self.revision = self.revision.saturating_add(1);
            self.publish_sketch_finish(event.clone());
            event.sequence = self.sketch_finish_sequence;
            if first.is_none() {
                first = Some(event);
            }
        }
        Ok(first)
    }

    pub(crate) fn run_nested_edit(&mut self, spec: &CommandSpec, params: &Value, continuation: bool) -> Result<Value> {
        self.with_sketch_exit_receiver(spec.id, |session| session.run_nested_edit_in_receiver(spec, params, continuation))
    }

    fn run_nested_edit_in_receiver(&mut self, spec: &CommandSpec, params: &Value, continuation: bool) -> Result<Value> {
        self.check_command(spec, params)?;
        let depth = self.edit_entry_depth;
        if continuation {
            self.edit_entry_depth = depth.saturating_add(1);
        }
        self.note_edit_entry(spec, params);
        let prepared = self.prepare_command_edit(spec, params);
        let result = match prepared {
            Err(error) => Err(error),
            Ok(event) => {
                let prepared = self.take_prepared_edit_source(spec, params);
                let event = event.or(prepared);
                let source = self.edit_source_sketch;
                if let Some(event) = event.as_ref().filter(|event| event.domain == SketchDomain::Model) {
                    self.edit_source_sketch = Some(event.sketch);
                }
                if self.command_starts_edit(spec, params) {
                    self.edit_entry_depth = self.edit_entry_depth.saturating_add(1);
                }
                let result = spec.allowed(self).map_err(|why| EngineError::Disabled(spec.id.into(), why)).and_then(|()| (spec.run)(self, params));
                self.edit_source_sketch = source;
                result.map(|value| finished_reply(value, event.as_ref()))
            }
        };
        self.edit_entry_depth = depth;
        result
    }
}

pub(crate) fn empty_geometry(sketch: &solidcraft_sketch::Sketch) -> bool {
    sketch.curves.is_empty() && sketch.wires.is_empty() && sketch.points.iter().all(|point| point.id == "origin")
}

/// Model-only catch-up prepared away from the window thread. No authoring state is adopted.
pub struct RefreshedSketchModel {
    source: Session,
    model: solidcraft_doc::Model,
    world_cache: Option<crate::WorldCache>,
    evaluated: bool,
}

impl crate::Ahead {
    /// Evaluate the restored Model document without a workspace command or host authorization.
    /// Selection, history, camera, drawing focus and event queues are not part of the handoff.
    pub fn refresh_sketch_model(mut self, quality: solidcraft_doc::display_quality::DisplayQuality) -> Result<RefreshedSketchModel> {
        let source = self.0.scratch();
        let stop = self.0.stop.clone();
        stop.within(|| {
            let evaluated = self.0.model.evaluate_shared(&self.0.doc);
            if solidcraft_doc::progress::cancelled() {
                return Err(EngineError::Other("model refresh cancelled".into()));
            }
            self.0.world_state()?.warm_display_at(quality, || !solidcraft_doc::progress::cancelled());
            if solidcraft_doc::progress::cancelled() {
                return Err(EngineError::Other("model refresh cancelled".into()));
            }
            let world_cache = self.0.world_cache.lock().ok().and_then(|cache| cache.clone());
            Ok(RefreshedSketchModel { source, model: self.0.model, world_cache, evaluated })
        })
    }
}

impl Session {
    /// Install only a still-current prepared Model cache. Stale work leaves pending catch-up set.
    pub fn accept_refreshed_sketch_model(&mut self, prepared: RefreshedSketchModel) -> bool {
        if !self.sketch_finish_pending
            || self.uid != prepared.source.uid
            || self.revision != prepared.source.revision
            || !self.same_document_context(&prepared.source)
            || !Arc::ptr_eq(&self.model.state(), &prepared.source.model.state())
        {
            return false;
        }
        self.model = prepared.model;
        if let Ok(mut cache) = self.world_cache.lock() {
            *cache = prepared.world_cache;
        }
        self.sketch_finish_pending = false;
        self.evaluations = self.evaluations.saturating_add(usize::from(prepared.evaluated));
        self.revision = self.revision.saturating_add(1);
        true
    }
}
