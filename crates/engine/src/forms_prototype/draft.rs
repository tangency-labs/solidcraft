//! Versioned authoring data kept only in autosaves.
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};

use super::{Form, Part, Snapshot, checked_finish_options, error};
use crate::doc::{Feature, FeatureKind, Operation, forms::Definition};
use crate::geom::subdivision::{MAX_LEVELS, pipe_network::SegmentGeometry};
use crate::{Result, Session};

const VERSION: u32 = 1;
const MAX_HISTORY: usize = 8;
const MAX_PARTS: usize = 16;
const MAX_BYTES: usize = solidcraft_io::MAX_FORM_DRAFT_BYTES;

pub(super) fn new_identity() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn accepted(form: &Form) -> &Form {
    form.tool_original.as_deref().unwrap_or(form)
}

pub(crate) fn revision(form: &Form) -> [u64; 3] {
    let form = accepted(form);
    [form.draft_identity, form.revision, form.finish_revision]
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Draft {
    version: u32,
    editing_feature: Option<u64>,
    authoring_component: u64,
    authoring_path: Option<Vec<u64>>,
    base_dirty: bool,
    #[serde(default)]
    finish_options: crate::schema::forms_prototype::FinishOptions,
    source: State,
    undo: Vec<State>,
    redo: Vec<State>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct State {
    definition: Definition,
    levels: usize,
    parts: BTreeMap<u64, Part>,
    consumed: BTreeMap<u64, Arc<Feature>>,
    next_part: u64,
    pipe_segments: Vec<SegmentGeometry>,
}

impl Draft {
    pub(crate) fn capture(form: &Form, base_dirty: bool) -> Self {
        let form = accepted(form);
        let history = |stack: &[Snapshot]| stack.iter().skip(stack.len().saturating_sub(MAX_HISTORY)).map(State::from_snapshot).collect();
        Self {
            version: VERSION,
            editing_feature: form.editing_feature,
            authoring_component: form.authoring_component,
            authoring_path: form.authoring_path.clone(),
            base_dirty,
            finish_options: form.finish_options.clone(),
            source: State {
                definition: form.definition(),
                levels: form.levels,
                parts: form.parts.clone(),
                consumed: form.consumed.clone(),
                next_part: form.next_part,
                pipe_segments: form.pipe_segments.clone(),
            },
            undo: history(&form.undo),
            redo: history(&form.redo),
        }
    }

    pub(crate) fn encode(mut self) -> Result<Vec<u8>> {
        let mut bytes = BoundedBytes(Vec::new());
        if let Err(first) = serde_json::to_writer(&mut bytes, &self) {
            if self.undo.is_empty() && self.redo.is_empty() {
                return Err(error(first));
            }
            // The current source takes precedence if several dense history entries
            // exceed the file budget. A failed current source leaves the old autosave.
            self.undo.clear();
            self.redo.clear();
            bytes.0.clear();
            serde_json::to_writer(&mut bytes, &self).map_err(error)?;
        }
        Ok(bytes.0)
    }

    pub(crate) fn restore(session: &Session, bytes: &[u8]) -> Result<(Form, bool)> {
        if bytes.len() > MAX_BYTES {
            return Err(error("Form recovery source exceeds its size limit"));
        }
        let draft: Self = serde_json::from_slice(bytes).map_err(error)?;
        if draft.version != VERSION || draft.undo.len() > MAX_HISTORY || draft.redo.len() > MAX_HISTORY {
            return Err(error("unsupported Form recovery version or history length"));
        }
        if draft.authoring_component != 0 && !session.doc.components.iter().any(|c| c.id == draft.authoring_component) {
            return Err(error("the recovered Form's authoring component no longer exists"));
        }
        if let Some(path) = &draft.authoring_path
            && (path.len() > 64 || session.occurrence_placement(path)?.component != draft.authoring_component)
        {
            return Err(error("the recovered Form's authoring placement no longer exists"));
        }
        if let Some(id) = draft.editing_feature
            && !session.doc.feature(id).is_some_and(|f| f.component == draft.authoring_component && matches!(f.kind, FeatureKind::Form { .. }))
        {
            return Err(error("the recovered Form's saved feature no longer exists"));
        }

        let mut form = draft.source.into_form(draft.editing_feature, true)?;
        form.authoring_component = draft.authoring_component;
        form.authoring_path = draft.authoring_path;
        form.finish_options = checked_finish_options(draft.finish_options)?;
        form.undo = Self::restore_history(draft.undo, draft.editing_feature)?;
        form.redo = Self::restore_history(draft.redo, draft.editing_feature)?;
        Ok((form, draft.base_dirty))
    }

    fn restore_history(states: Vec<State>, editing: Option<u64>) -> Result<Vec<Snapshot>> {
        states
            .into_iter()
            .map(|state| {
                let levels = state.levels;
                let mut form = state.into_form(editing, false)?;
                form.levels = levels;
                Ok(form.snapshot())
            })
            .collect()
    }
}

impl State {
    fn from_snapshot(s: &Snapshot) -> Self {
        Self {
            definition: Definition {
                imported: s.imported.clone(),
                pipe: s.pipe.clone(),
                profile: s.profile.clone(),
                matching: s.matching.clone(),
                placement: s.placement.clone(),
                cage: s.cage.clone(),
                parameters: s.parameters.clone(),
                symmetry: s.symmetry.clone(),
                hierarchy: s.hierarchy.clone(),
            },
            levels: s.levels,
            parts: s.parts.clone(),
            consumed: s.consumed.clone(),
            next_part: s.next_part,
            pipe_segments: s.pipe_segments.clone(),
        }
    }

    fn into_form(self, editing: Option<u64>, full_preview: bool) -> Result<Form> {
        if self.levels > MAX_LEVELS
            || self.parts.len() > MAX_PARTS
            || self.consumed.len() > MAX_PARTS
            || self.next_part == 0
            || self.next_part == u64::MAX
            || self
                .parts
                .iter()
                .any(|(&id, part)| id == 0 || id >= self.next_part || part.name.is_empty() || part.name.len() > 256 || part.name.contains('\0'))
            || self.consumed.iter().any(|(&id, feature)| {
                id == 0 || id != feature.id || editing == Some(id) || !matches!(feature.kind, FeatureKind::Form { operation: Operation::NewBody, .. })
            })
        {
            return Err(error("invalid staged objects in Form recovery source"));
        }
        if self.pipe_segments.len() > 128
            || self.pipe_segments.iter().any(|segment| {
                segment.key.len() > 4096
                    || !segment.diameter.is_finite()
                    || !(1..=20).contains(&segment.samples)
                    || segment.path.len() > 40_000
                    || segment.path.iter().any(|point| !point.is_finite())
                    || segment.faces.len() > 30_000
            })
        {
            return Err(error("invalid pipe segments in Form recovery source"));
        }
        for part in self.parts.values() {
            part.definition.placement.validate()?;
            part.definition.parameters.validate().map_err(error)?;
            part.definition.cage.validate().map_err(error)?;
        }
        let mut form = Form::from_definition(self.definition, if full_preview { self.levels } else { 0 }, editing)?;
        form.parts = self.parts;
        form.consumed = self.consumed;
        form.next_part = self.next_part;
        form.pipe_segments = self.pipe_segments;
        Ok(form)
    }
}

struct BoundedBytes(Vec<u8>);

impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_BYTES.saturating_sub(self.0.len()) {
            return Err(io::Error::other("Form recovery source exceeds its size limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
