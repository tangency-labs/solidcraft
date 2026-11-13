//! Immutable inputs for the read-only Form CAD command and its native worker.
use crate::{EngineError, Result, Session, doc, schema::forms_prototype as params};
use std::hash::{Hash, Hasher};
use std::{collections::BTreeMap, sync::Arc};

mod browser;
pub use browser::BrowserCheck;

/// The current world frame of the exact authoring occurrence, with the
/// compatible first-occurrence fallback for older or missing saved paths.
pub(crate) fn authoring_frame(document: &doc::Document, moves: &BTreeMap<u64, doc::Mat>, component: u64, path: Option<&[u64]>) -> doc::Mat {
    let frame = |path: &[u64]| -> Option<(u64, doc::Mat)> {
        if path.is_empty() || path.len() > 64 {
            return None;
        }
        let mut component = 0;
        let mut world = doc::IDENTITY;
        let mut seen = std::collections::BTreeSet::new();
        for id in path {
            if !seen.insert(*id) {
                return None;
            }
            let occurrence = document.occurrences.iter().find(|o| o.id == *id && o.parent == component)?;
            if !document.components.iter().any(|c| c.id == occurrence.component) {
                return None;
            }
            world = crate::kernel::affine::then(moves.get(id).unwrap_or(&occurrence.transform), &world).ok()?;
            component = occurrence.component;
        }
        Some((component, world))
    };
    if let Some(path) = path
        && let Some((owner, world)) = frame(path)
        && owner == component
    {
        return world;
    }
    frame(&document.first_path(component)).map_or(doc::IDENTITY, |(_, world)| world)
}

/// Pipe follows the saved source, including occurrence placement. The
/// temporary authoring frame maps world picks into that saved source state.
/// Applying pending moves to external bodies here would make Check and Finish
/// depend on placements absent from the saved Form's timeline replay.
pub(crate) fn pipe_source_state<'a>(
    document: &doc::Document,
    before: &'a doc::ModelState,
    component: u64,
    frame: doc::Mat,
) -> Result<std::borrow::Cow<'a, doc::ModelState>> {
    Ok(doc::state_in_frame_at(document, before, component, frame)?)
}

/// Source identity for read-query preparation, independent of display settings.
/// Computing it inspects bounded editor metadata and pending occurrence moves,
/// never preview buffers, source geometry or editor history.
#[derive(Clone)]
pub struct Stamp {
    document: Arc<doc::Document>,
    document_id: u64,
    authoring: u64,
    source: (u64, u64),
    finish_revision: u64,
    component: u64,
    path: Option<Vec<u64>>,
    editing: Option<u64>,
    parts: Vec<(u64, Arc<doc::forms::Definition>)>,
    options: params::FinishOptions,
    preview: bool,
    repair: bool,
    moves: u64,
}
impl PartialEq for Stamp {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.document, &other.document)
            && self.document_id == other.document_id
            && self.authoring == other.authoring
            && self.source == other.source
            && self.finish_revision == other.finish_revision
            && self.component == other.component
            && self.path == other.path
            && self.editing == other.editing
            && self.parts.len() == other.parts.len()
            && self.parts.iter().zip(&other.parts).all(|((a_id, a), (b_id, b))| a_id == b_id && Arc::ptr_eq(a, b))
            && self.options == other.options
            && self.preview == other.preview
            && self.repair == other.repair
            && self.moves == other.moves
    }
}
impl Stamp {
    pub fn current(session: &Session) -> Option<Self> {
        let form = session.forms_prototype.as_ref()?;
        let mut moves = std::collections::hash_map::DefaultHasher::new();
        session.pending_moves.len().hash(&mut moves);
        for (id, matrix) in &session.pending_moves {
            id.hash(&mut moves);
            for value in matrix.iter().flatten() {
                value.to_bits().hash(&mut moves);
            }
        }
        Some(Self {
            document: session.doc.clone(),
            document_id: session.uid,
            authoring: session.authoring_revision(),
            source: form.source_revision(),
            finish_revision: form.finish_revision,
            component: form.authoring_component,
            path: form.authoring_path.clone(),
            editing: form.editing_feature,
            parts: form.parts.iter().map(|(id, part)| (*id, part.definition.clone())).collect(),
            options: form.finish_options.clone(),
            preview: form.has_tool_preview(),
            repair: form.repair.is_some(),
            moves: moves.finish(),
        })
    }
    /// Context-only changes can reuse the producer's immutable definition.
    pub fn same_definition(&self, other: &Self) -> bool {
        self.source == other.source
    }
}

/// The source document in the Form author's frame, without an editor or its history.
#[derive(Clone)]
pub struct Context {
    pub(crate) document: Arc<doc::Document>,
    pub(crate) before: Arc<doc::ModelState>,
    moves: Arc<BTreeMap<u64, doc::Mat>>,
    component: u64,
    path: Option<Arc<[u64]>>,
    editing: Option<u64>,
    staged: usize,
    tolerance: Option<Arc<str>>,
    unavailable: bool,
}
impl Context {
    /// Prepare alongside a source producer, never in a window-thread button handler.
    /// Only source context is copied: no Form, mesh, evaluator or history is cloned.
    pub fn capture(session: &Session) -> Result<Self> {
        let form = session.forms_prototype.as_ref().ok_or_else(|| error("create a Form first"))?;
        Ok(Self {
            document: session.doc.clone(),
            before: form.editing_feature.map_or_else(|| session.model.state(), |id| session.model.state_before(id)),
            moves: Arc::new(session.pending_moves.clone()),
            component: form.authoring_component,
            path: form.authoring_path.as_deref().map(Arc::from),
            editing: form.editing_feature,
            staged: form.parts.len(),
            tolerance: form.finish_options.tolerance.as_deref().map(Arc::from),
            unavailable: form.has_tool_preview() || form.repair.is_some(),
        })
    }
    /// Bind the producer's source. The owner must qualify its revision/context before use.
    pub fn with_source(self: Arc<Self>, source: Arc<doc::forms::Definition>) -> Input {
        Input { context: self, source }
    }
    pub(crate) fn frame(&self) -> doc::Mat {
        authoring_frame(&self.document, &self.moves, self.component, self.path.as_deref())
    }
    fn world(&self) -> Result<doc::ModelState> {
        let mut document = std::borrow::Cow::Borrowed(self.document.as_ref());
        if !self.moves.is_empty() {
            for occurrence in &mut document.to_mut().occurrences {
                if let Some(matrix) = self.moves.get(&occurrence.id) {
                    occurrence.transform = *matrix;
                }
            }
        }
        Ok(doc::world_state(&document, &self.before)?)
    }
    fn pipe(&self, definition: &mut doc::forms::Definition) -> Result<()> {
        let Some(pipe) = &definition.pipe else { return Ok(()) };
        let frame = self.frame();
        let inverse = crate::kernel::affine::inverse(&frame).map_err(|_| error("the pipe author frame is not invertible"))?;
        let state = pipe_source_state(&self.document, &self.before, self.component, frame)?;
        let mut local = pipe.as_ref().clone();
        local.source.transform_picks(&inverse);
        definition.cage = local.recompute(&definition.cage, &state)?;
        Ok(())
    }
    fn tolerance(&self, requested: Option<f64>) -> Result<f64> {
        let saved = self.editing.and_then(|id| self.document.feature(id)).and_then(|feature| match &feature.kind {
            doc::FeatureKind::Form { tolerance, .. } => Some(tolerance.as_str()),
            _ => None,
        });
        let value = match requested {
            Some(value) => Some(value),
            None => match self.tolerance.as_deref().or(saved) {
                Some(text) => {
                    let text = super::validate_tolerance_text(text)?;
                    text.parse::<f64>().ok().or_else(|| self.document.eval(text, doc::expr::Kind::Length).ok())
                }
                None => Some(0.01),
            },
        };
        check_tolerance(value)
    }
}

/// A qualified producer snapshot. Cloning it shares source and document buffers.
#[derive(Clone)]
pub struct Input {
    pub(crate) context: Arc<Context>,
    pub(crate) source: Arc<doc::forms::Definition>,
}
impl Input {
    pub fn definition(&self) -> Arc<doc::forms::Definition> {
        self.source.clone()
    }
    /// Command adapter. Native UI callers reuse a previously prepared producer input.
    pub fn capture(session: &Session) -> Result<Self> {
        let context = Arc::new(Context::capture(session)?);
        let form = session.forms_prototype.as_ref().ok_or_else(|| error("create a Form first"))?;
        Ok(context.with_source(Arc::new(form.definition())))
    }
    /// Canonical read-only implementation used by `form.prototype.validate`.
    pub fn check(&self, args: &params::Validate) -> Result<params::ValidationReply> {
        if self.context.unavailable {
            return Err(error("apply or cancel the active Form tool, and repair its support, before checking the Form"));
        }
        let tolerance = self.context.tolerance(args.tolerance)?;
        let mut reply = initial_reply(tolerance, self.context.staged);
        let source = (|| -> Result<_> {
            let mut definition = self.source.as_ref().clone();
            self.context.pipe(&mut definition)?;
            let state = if definition.profile.is_some() || definition.matching.is_some() { Some(self.context.world()?) } else { None };
            if let Some(matching) = &definition.matching {
                let state = state.as_ref().ok_or_else(|| error("Form checking lost its Match source document"))?;
                definition = matching.recompute(&definition, state, tolerance)?.0;
            }
            if let Some(profile) = &definition.profile {
                let state = state.as_ref().ok_or_else(|| error("Form checking lost its Surface source document"))?;
                let (profile, cage) = profile.recompute(&definition, state)?;
                definition.profile = Some(Box::new(profile));
                definition.cage = cage;
            }
            Ok((definition, state))
        })();
        let (definition, state) = match source {
            Ok(source) => source,
            Err(error) => {
                reply.conversion = check_result(params::CheckStatus::Failed, Some(error.to_string()));
                return Ok(reply);
            }
        };
        Ok(check_fit(reply, &definition, state.as_ref()))
    }
}
fn check_tolerance(value: Option<f64>) -> Result<f64> {
    value
        .filter(|value| value.is_finite() && (1e-6..=1.0).contains(value))
        .ok_or_else(|| error("Form CAD tolerance must be between 0.000001 and 1 mm"))
}
fn initial_reply(tolerance: f64, staged: usize) -> params::ValidationReply {
    params::ValidationReply {
        tolerance_mm: tolerance,
        staged_forms_not_checked: staged,
        stage: params::CheckStage::Source,
        shape: None,
        fit: None,
        conversion: check_result(params::CheckStatus::NotRun, None),
        record: check_result(params::CheckStatus::NotRun, None),
        smlib: check_result(params::CheckStatus::NotRun, None),
    }
}
fn check_fit(mut reply: params::ValidationReply, definition: &doc::forms::Definition, state: Option<&doc::ModelState>) -> params::ValidationReply {
    let tolerance = reply.tolerance_mm;
    reply.stage = params::CheckStage::Fitting;
    let converted = (|| -> Result<_> {
        if let Some(profile) = &definition.profile {
            let state = state.ok_or_else(|| error("Form checking lost its Surface source document"))?;
            let fit = profile.fit(state, &definition.cage, &definition.placement, tolerance)?;
            let summary = params::SampledFit {
                sampled_deviation_mm: fit.report.gap.max(fit.report.point_gap),
                basis: params::FitBasis::BoundaryAndGuideSamples,
                patches: fit.patch.face_count(),
                subdivision_level: None,
            };
            return Ok((fit.solid, summary));
        }
        let (body, fit) = doc::forms::convert(definition, tolerance)?;
        Ok((
            body,
            params::SampledFit {
                sampled_deviation_mm: fit.sampled_deviation_mm,
                basis: params::FitBasis::SubdivisionSamples,
                patches: fit.patches,
                subdivision_level: Some(fit.subdivision_level),
            },
        ))
    })();
    let (body, fit) = match converted {
        Ok(converted) => converted,
        Err(error) => {
            reply.conversion = check_result(params::CheckStatus::Failed, Some(error.to_string()));
            return reply;
        }
    };
    if !fit.sampled_deviation_mm.is_finite() || fit.sampled_deviation_mm < 0.0 {
        reply.conversion = check_result(params::CheckStatus::Failed, Some("Form fitting returned an invalid sampled deviation".into()));
        return reply;
    }
    reply.shape = Some(if body.is_surface() { params::CheckedShape::Surface } else { params::CheckedShape::Solid });
    reply.fit = Some(fit);
    reply.conversion = check_result(params::CheckStatus::Passed, None);
    (reply.record, reply.smlib) = validity_checks(doc::kernel::validity(&body));
    reply.stage = params::CheckStage::Complete;
    reply
}
fn error(message: &str) -> EngineError {
    EngineError::Other(message.into())
}
fn check_result(status: params::CheckStatus, message: Option<String>) -> params::CheckResult {
    params::CheckResult { status, message }
}
pub(crate) fn validity_checks(validity: doc::kernel::Validity) -> (params::CheckResult, params::CheckResult) {
    use params::CheckStatus::{Failed, NotRun, NotSupported, Passed};
    let record = match validity.record {
        Ok(()) => check_result(Passed, None),
        Err(message) => check_result(Failed, Some(message)),
    };
    let smlib = match validity.smlib {
        Some(doc::kernel::SmlibCheck::Valid) => check_result(Passed, None),
        Some(doc::kernel::SmlibCheck::Invalid(message)) => check_result(Failed, Some(message)),
        Some(doc::kernel::SmlibCheck::NotSupported(message)) => check_result(NotSupported, Some(message)),
        None => check_result(NotRun, None),
    };
    (record, smlib)
}
