//! A qualified Form-only edit computed beside the window.
use super::{Form, error, validation::Stamp};
use crate::{CommandCheck, EngineError, Result, Session, doc, schema};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};

mod browser;
pub use browser::BrowserTransform;

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
    /// Derived faces must keep their fitted CAD seams valid after an edit.
    pub fn needs_fitted_transform(&self) -> bool {
        self.hierarchy.as_ref().is_some_and(|h| h.has_boundary_collapse() || h.has_quad_cap())
    }

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

/// The fit tolerance for edits to derived faces. Ordinary Form transforms keep
/// their existing fast path.
pub(crate) fn derived_edit_tolerance(session: &Session) -> Result<Option<f64>> {
    let form = session.forms_prototype.as_ref().ok_or_else(|| error("no Form is being edited"))?;
    if !form.needs_fitted_transform() {
        return Ok(None);
    }
    let saved = form.editing_feature.and_then(|id| session.doc.feature(id)).and_then(|feature| match &feature.kind {
        doc::FeatureKind::Form { tolerance, .. } => Some(tolerance.as_str()),
        _ => None,
    });
    let text = form.finish_options.tolerance.as_deref().or(saved).unwrap_or("0.01 mm");
    super::finish_tolerance(session, text).map(Some)
}

pub(crate) fn qualify_derived_edit(form: &Form, tolerance: f64) -> Result<()> {
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
    form: Box<Form>,
    space: FormTargetSpace,
    params: Value,
    derived_edit_tolerance: Option<f64>,
    stop: solidcraft_kernel::Stop,
}

/// The isolated result of an existing Form target command. Only the Form can
/// be accepted; it cannot replace the live document, model or selection.
pub struct PreparedFormTransform {
    source: FormTransformStamp,
    form: Option<Box<Form>>,
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
        Ok(FormTransformJob {
            source,
            form: form.clone(),
            space,
            params,
            derived_edit_tolerance: derived_edit_tolerance(self)?,
            stop: self.stop_token(),
        })
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
        let Self { source, mut form, space, params, derived_edit_tolerance, stop } = self;
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
                    if let Some(tolerance) = derived_edit_tolerance
                        && form.source_revision() != revision
                    {
                        qualify_derived_edit(&form, tolerance)?;
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

    pub(super) fn capped_form() -> (Session, usize) {
        let mut session = Session::default();
        session
            .execute("form.prototype.create", &serde_json::json!({"primitive":"face","dimensions":[8,8,1],"segments":[4,4,1],"levels":1}))
            .unwrap();
        session.execute("form.prototype.refine", &serde_json::json!({"faces":[0],"counts":[2,2]})).unwrap();
        let form = session.forms_prototype.as_ref().unwrap();
        let fine = *form.hierarchy.as_ref().unwrap().controls.iter().find(|(_, depth)| **depth == 1).unwrap().0;
        let point = form.control_positions()[fine] + Vec3::Z * 0.01;
        session.execute("form.prototype.set_vertices", &serde_json::json!({"vertices":[fine],"positions":[[point.x,point.y,point.z]]})).unwrap();
        let frame = doc::rigid(Vec3::new(3.0, -2.0, 4.0), Vec3::ZERO, Vec3::Z, 0.2);
        session.execute("form.prototype.transform", &serde_json::json!({"matrix":frame})).unwrap();
        let edge: [usize; 2] = session.forms_prototype.as_ref().unwrap().logical.faces[10][..2].try_into().unwrap();
        session.execute("form.prototype.topology", &serde_json::json!({"tool":"delete_faces","faces":[10]})).unwrap();
        session
            .execute("form.prototype.topology", &serde_json::json!({"tool":"fill_hole","edges":[edge],"fill_mode":"new_cap","tolerance":0.05}))
            .unwrap();
        session.execute("form.prototype.finish_options", &serde_json::json!({"tolerance":"0.05 mm"})).unwrap();
        let cap = session.forms_prototype.as_ref().unwrap().hierarchy.as_ref().unwrap().quad_cap_face().unwrap();
        (session, cap)
    }

    #[test]
    fn cap_transform_workers_match_commands_preserve_fine_detail_and_round_trip() {
        let (source, cap) = capped_form();
        let source_form = source.forms_prototype.as_ref().unwrap();
        assert!(source_form.needs_fitted_transform());
        let original = source_form.definition();
        let hierarchy = original.hierarchy.as_ref().unwrap();
        let rim = hierarchy.validated_quad_cap().unwrap();
        let ids = source_form.logical.faces[cap].clone();
        assert_eq!(ids.len(), 4);
        assert!(ids.iter().all(|id| hierarchy.controls[id] == 0), "the fixture edits a coarse rim");
        assert!(hierarchy.details[1].iter().any(|detail| detail.delta.len() > 0.0), "the fixture contains a real unrelated fine sculpt");
        let modes = [
            ([0.0, 0.0, 0.04], [0.0; 3], [1.0; 3]),
            ([0.0; 3], [0.0, 0.0, 0.025], [1.0; 3]),
            ([0.0; 3], [0.01, 0.0, 0.0], [1.0; 3]),
            ([0.0; 3], [0.0; 3], [1.02, 1.0, 1.0]),
            ([0.0; 3], [0.0; 3], [1.01, 0.99, 1.0]),
            ([0.0; 3], [0.0; 3], [1.015; 3]),
        ];
        for space in [FormTargetSpace::Control, FormTargetSpace::Surface] {
            let points = match space {
                FormTargetSpace::Control => source_form.control_positions(),
                FormTargetSpace::Surface => source_form.surface.vertices.clone(),
            };
            let center = ids.iter().map(|&id| points[id]).fold(Vec3::ZERO, |sum, point| sum + point) / ids.len() as f64;
            for (translation, rotation, scale) in modes {
                let targets: Vec<_> = ids
                    .iter()
                    .map(|&id| {
                        let point = solidcraft_kernel::services::form_transform::numeric(
                            points[id],
                            center,
                            source_form.placement.axes,
                            translation,
                            rotation,
                            scale,
                            1.0,
                        )
                        .unwrap();
                        [point.x, point.y, point.z]
                    })
                    .collect();
                let params = serde_json::json!({"vertices":ids,"positions":targets});
                let mut worker = source.scratch();
                let mut direct = source.scratch();
                let undo = worker.forms_prototype.as_ref().unwrap().inspect()["undo"].as_u64().unwrap();
                let document = worker.doc.clone();
                let prepared = worker.prepare_form_transform(space, params.clone()).unwrap().compute().unwrap();
                let expected = direct.execute(space.command(), &params).unwrap();
                let (changed, reply) = worker.accept_form_transform(prepared).unwrap();
                assert!(changed);
                assert_eq!(reply, expected);
                assert!(Arc::ptr_eq(&worker.doc, &document));
                let form = worker.forms_prototype.as_ref().unwrap();
                assert_eq!(form.definition(), direct.forms_prototype.as_ref().unwrap().definition());
                assert_eq!(form.inspect()["undo"], undo + 1);
                let after = form.definition();
                let edited = after.hierarchy.as_ref().unwrap();
                assert_eq!(edited.base, hierarchy.base);
                assert_eq!(edited.controls, hierarchy.controls);
                for (birth, details) in hierarchy.details.iter().enumerate().skip(1) {
                    assert_eq!(edited.details.get(birth), Some(details), "editing the cap's coarse rim must retain unrelated fine detail");
                }
                assert_eq!(edited.validated_quad_cap().unwrap(), rim);
                let actual = match space {
                    FormTargetSpace::Control => form.control_positions(),
                    FormTargetSpace::Surface => form.surface.vertices.clone(),
                };
                for (&id, target) in ids.iter().zip(&targets) {
                    assert!(
                        actual[id].dist(Vec3::new(target[0], target[1], target[2])) < 1e-7,
                        "{}: {translation:?} {rotation:?} {scale:?}",
                        space.command()
                    );
                }
                let (body, report) = doc::forms::convert(&after, 0.05).unwrap();
                assert!(!body.is_mesh() && crate::kernel::validity(&body).ok());
                assert!(report.patch_roots.contains(&cap));
                worker.execute("form.prototype.undo", &serde_json::json!({})).unwrap();
                assert_eq!(worker.forms_prototype.as_ref().unwrap().definition(), original);
                worker.execute("form.prototype.redo", &serde_json::json!({})).unwrap();
                assert_eq!(worker.forms_prototype.as_ref().unwrap().definition(), after);
                let restored = crate::recovery::restore_bytes(&crate::recovery::capture_bytes(&worker).unwrap(), None).unwrap();
                assert_eq!(restored.forms_prototype.as_ref().unwrap().definition(), after);
                let feature = worker.execute("form.prototype.commit", &serde_json::json!({"tolerance":0.05})).unwrap()["feature"].as_u64().unwrap();
                let bytes = solidcraft_io::write_design(worker.doc.as_ref()).unwrap();
                let mut reopened = Session::new(solidcraft_io::read_design(&bytes).unwrap());
                reopened.execute("form.edit", &serde_json::json!({"feature":feature})).unwrap();
                assert_eq!(reopened.forms_prototype.as_ref().unwrap().definition(), after);
                assert_eq!(reopened.forms_prototype.as_ref().unwrap().logical.faces[cap], ids);
            }
        }
    }

    #[test]
    fn invalid_cap_transform_is_refused_before_history_or_recovery_changes() {
        let (mut session, cap) = capped_form();
        let form = session.forms_prototype.as_ref().unwrap();
        let ids = form.logical.faces[cap].clone();
        let center = form.surface.vertices[ids[0]];
        let targets = vec![[center.x, center.y, center.z]; ids.len()];
        let params = serde_json::json!({"vertices":ids,"positions":targets});
        let typed = serde_json::from_value(params.clone()).unwrap();
        let mut unqualified = form.clone();
        unqualified.apply_transform_targets(&typed, FormTargetSpace::Surface).unwrap();
        assert!(qualify_derived_edit(&unqualified, 0.05).is_err(), "the refusal must come from native cap qualification");
        let original = form.definition();
        let before = form.inspect();
        let recovery = crate::recovery::capture_bytes(&session).unwrap();
        let document = session.doc.clone();
        let job = session.prepare_form_transform(FormTargetSpace::Surface, params.clone()).unwrap();
        assert!(job.compute().is_err());
        assert_eq!(session.forms_prototype.as_ref().unwrap().definition(), original);
        assert_eq!(session.forms_prototype.as_ref().unwrap().inspect(), before);
        assert_eq!(crate::recovery::capture_bytes(&session).unwrap(), recovery);
        assert!(session.execute(FormTargetSpace::Surface.command(), &params).is_err());
        assert_eq!(session.forms_prototype.as_ref().unwrap().definition(), original);
        assert_eq!(session.forms_prototype.as_ref().unwrap().inspect(), before);
        assert_eq!(crate::recovery::capture_bytes(&session).unwrap(), recovery);
        assert!(Arc::ptr_eq(&session.doc, &document));
        let targets: Vec<_> = ids
            .iter()
            .map(|&id| {
                let point = session.forms_prototype.as_ref().unwrap().surface.vertices[id];
                [point.x, point.y, point.z + 0.02]
            })
            .collect();
        let ready = session
            .prepare_form_transform(FormTargetSpace::Surface, serde_json::json!({"vertices":ids,"positions":targets}))
            .unwrap()
            .compute()
            .unwrap();
        session.execute("form.prototype.finish_options", &serde_json::json!({"tolerance":"0.06 mm"})).unwrap();
        let changed_context = crate::recovery::capture_bytes(&session).unwrap();
        assert!(session.accept_form_transform(ready).is_err(), "a cap fit cannot be accepted under a different tolerance");
        assert_eq!(crate::recovery::capture_bytes(&session).unwrap(), changed_context);
    }

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
