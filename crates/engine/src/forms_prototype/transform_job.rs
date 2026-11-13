//! A qualified Form-only edit computed beside the window.
use super::{Form, error, validation::Stamp};
use crate::{CommandCheck, EngineError, Result, Session, doc, schema};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};

/// Which points the existing Form target command addresses.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FormTargetSpace {
    Control,
    Surface,
}
impl FormTargetSpace {
    pub fn command(self) -> &'static str {
        match self {
            Self::Control => "form.prototype.set_vertices",
            Self::Surface => "form.prototype.set_surface_vertices",
        }
    }
}

impl Form {
    pub(crate) fn require_editable(&self) -> Result<()> {
        if self.tool_original.is_some() {
            return Err(error("apply or cancel the active Form tool first"));
        }
        if self.repair.is_some() {
            return Err(error("reattach the retained Form support and Apply, or Cancel"));
        }
        Ok(())
    }

    pub(crate) fn apply_transform_targets(&mut self, params: &schema::forms_prototype::SetVertices, space: FormTargetSpace) -> Result<()> {
        match space {
            FormTargetSpace::Control => self.set(&params.vertices, &params.positions, params.checkpoint),
            FormTargetSpace::Surface => self.set_surface(&params.vertices, &params.positions, params.checkpoint),
        }
    }
}

/// The fit tolerance for edits to a retained Collapse. Other Form transforms
/// keep their existing fast path.
pub(crate) fn collapse_tolerance(session: &Session) -> Result<Option<f64>> {
    let form = session.forms_prototype.as_ref().ok_or_else(|| error("no Form is being edited"))?;
    if !form.hierarchy.as_ref().is_some_and(|h| h.has_boundary_collapse()) {
        return Ok(None);
    }
    let saved = form.editing_feature.and_then(|id| session.doc.feature(id)).and_then(|feature| match &feature.kind {
        doc::FeatureKind::Form { tolerance, .. } => Some(tolerance.as_str()),
        _ => None,
    });
    let text = form.finish_options.tolerance.as_deref().or(saved).unwrap_or("0.01 mm");
    super::finish_tolerance(session, text).map(Some)
}

pub(crate) fn qualify_collapse_edit(form: &Form, tolerance: f64) -> Result<()> {
    doc::forms::convert(&form.definition(), tolerance)?;
    Ok(())
}

/// Current authoring source and live command context for one Form gesture.
#[derive(Clone)]
pub struct FormTransformStamp {
    stamp: Stamp,
    active_component: u64,
    active_path: Option<Vec<u64>>,
    active_sketch: Option<u64>,
    moves: BTreeMap<u64, doc::Mat>,
    param_check: schema::ParamCheck,
    host: Option<CommandCheck>,
}

impl FormTransformStamp {
    pub fn capture(session: &Session) -> Option<Self> {
        Some(Self {
            stamp: Stamp::current(session)?,
            active_component: session.active_component,
            active_path: session.active_occurrence_path.clone(),
            active_sketch: session.active_sketch,
            moves: session.pending_moves.clone(),
            param_check: session.param_check,
            host: session.command_check(),
        })
    }

    pub fn is_current(&self, session: &Session) -> bool {
        let host = session.command_check();
        self.active_component == session.active_component
            && self.active_path == session.active_occurrence_path
            && self.active_sketch == session.active_sketch
            && self.moves == session.pending_moves
            && self.param_check == session.param_check
            && match (&self.host, host) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, &b),
                _ => false,
            }
            && Stamp::current(session).is_some_and(|stamp| stamp == self.stamp)
    }
}

/// An owned sample of an absolute Move, Rotate or Scale target. It holds the
/// current draft source but shares its immutable display and history storage.
pub struct FormTransformJob {
    source: FormTransformStamp,
    form: Form,
    space: FormTargetSpace,
    params: Value,
    collapse_tolerance: Option<f64>,
    stop: solidcraft_kernel::Stop,
}

/// The isolated result of an existing Form target command. Only the Form can
/// be accepted; it cannot replace the live document, model or selection.
pub struct PreparedFormTransform {
    source: FormTransformStamp,
    form: Option<Form>,
    space: FormTargetSpace,
    params: Value,
    reply: Value,
}

impl Session {
    /// Capture a target sample on the owner thread, with the live host's
    /// command check. Absolute targets and checkpoint flags have the same
    /// meaning as the existing target commands.
    pub fn prepare_form_transform(&self, space: FormTargetSpace, params: Value) -> Result<FormTransformJob> {
        let id = space.command();
        let spec = crate::find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.into()))?;
        (spec.enabled)(self).map_err(|why| EngineError::Disabled(id.into(), why))?;
        self.check_command(spec, &params)?;
        let form = self.forms_prototype.as_ref().ok_or_else(|| error("create a Form first"))?;
        let source = FormTransformStamp::capture(self).ok_or_else(|| error("create a Form first"))?;
        Ok(FormTransformJob { source, form: form.clone(), space, params, collapse_tolerance: collapse_tolerance(self)?, stop: self.stop_token() })
    }

    /// Apply a still-current result, leaving document history, moves, view,
    /// model and selection in place. The boolean records a real authored
    /// change, so a constrained no-op cannot consume the first checkpoint.
    pub fn accept_form_transform(&mut self, prepared: PreparedFormTransform) -> Result<(bool, Value)> {
        if self.stop_token().is_stopped() || !prepared.source.is_current(self) {
            return Err(error("the Form changed while its transform was computing"));
        }
        let changed = prepared.form.is_some();
        if let Some(form) = prepared.form {
            let before = self.forms_prototype.replace(form);
            if self.checkpoints.last().is_some_and(|checkpoint| checkpoint.form.is_none()) {
                self.store_form_at_checkpoints(before);
            }
        }
        crate::crash::accepted_command(self, &self.doc, &self.doc, prepared.space.command(), &prepared.params, changed);
        Ok((changed, prepared.reply))
    }
}

impl FormTransformJob {
    /// Run the exact target command on owned Form data. Discarding a result
    /// leaves the live Form and its history untouched.
    pub fn compute(self) -> Result<PreparedFormTransform> {
        let Self { source, mut form, space, params, collapse_tolerance, stop } = self;
        let id = space.command();
        if stop.is_stopped() {
            return Err(error("Form transform stopped"));
        }
        let revision = form.source_revision();
        let empty = Value::Object(Default::default());
        let given = if params.is_null() { &empty } else { &params };
        let _span = solidcraft_profile::span("command", id);
        let reply = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            stop.within(|| {
                let _scope = crate::crash::Scope::enter();
                schema::run_detached_checked(source.param_check, given, id, || {
                    let targets = serde_json::from_value(given.clone()).map_err(|e| error(e.to_string()))?;
                    form.require_editable()?;
                    form.apply_transform_targets(&targets, space)?;
                    if let Some(tolerance) = collapse_tolerance
                        && form.source_revision() != revision
                    {
                        qualify_collapse_edit(&form, tolerance)?;
                    }
                    Ok(form.inspect())
                })
            })
        }))
        .map_err(|panic| {
            let message = panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned()).unwrap_or_default();
            EngineError::Internal(id.into(), message)
        })??;
        if stop.is_stopped() {
            return Err(error("Form transform stopped"));
        }
        let form = (form.source_revision() != revision).then_some(form);
        Ok(PreparedFormTransform { source, form, space, params, reply })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::Vec3;
    use solidcraft_geom::Vec3Geom;
    use std::sync::Arc;

    #[test]
    fn repeated_refined_surface_targets_reuse_the_linear_response_and_preserve_undo() {
        let mut session = Session::default();
        session
            .execute("form.prototype.create", &serde_json::json!({"primitive":"face","dimensions":[12,12,1],"segments":[3,3,1],"levels":1}))
            .unwrap();
        session.execute("form.prototype.refine", &serde_json::json!({"faces":[4],"counts":[2,2]})).unwrap();
        let form = session.forms_prototype.as_mut().unwrap();
        let ids: Vec<_> = form.hierarchy.as_ref().unwrap().controls.iter().filter_map(|(&id, &level)| (level == 1).then_some(id)).take(3).collect();
        assert_eq!(ids.len(), 3);
        let source = form.definition();
        let undo = form.inspect()["undo"].as_u64().unwrap();
        let initial: Vec<_> = ids.iter().map(|&id| form.surface.vertices[id]).collect();
        let targets = |amount| initial.iter().map(|p| [p.x, p.y, p.z + amount]).collect::<Vec<_>>();
        form.set_surface(&ids, &targets(0.2), true).unwrap();
        let response = form.response.as_ref().unwrap().clone();
        let mut cold = form.clone();
        cold.response = None;
        form.set_surface(&ids, &targets(0.4), false).unwrap();
        cold.set_surface(&ids, &targets(0.4), false).unwrap();
        assert!(Arc::ptr_eq(form.response.as_ref().unwrap(), &response));
        assert_eq!(form.definition(), cold.definition());
        for (&id, target) in ids.iter().zip(targets(0.4)) {
            assert!(form.surface.vertices[id].dist(Vec3::new(target[0], target[1], target[2])) < 1e-8);
        }
        assert_eq!(form.inspect()["undo"], undo + 1);
        let p = form.control_positions()[ids[0]];
        form.set(&ids[..1], &[[p.x, p.y, p.z + 0.1]], false).unwrap();
        assert!(!Arc::ptr_eq(form.response.as_ref().unwrap(), &response));
        form.history(false).unwrap();
        assert!(form.response.is_none());
        assert_eq!(form.definition(), source);
    }
}
