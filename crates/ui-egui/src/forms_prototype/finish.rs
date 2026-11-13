//! Prepare expensive Form commands away from the native window thread.
use super::*;

#[cfg(not(target_arch = "wasm32"))]
pub(super) struct WorkerPermit(std::sync::Arc<std::sync::atomic::AtomicBool>);
#[cfg(not(target_arch = "wasm32"))]
impl WorkerPermit {
    pub(super) fn acquire(gate: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Option<Self> {
        use std::sync::atomic::Ordering;
        gate.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).ok().map(|_| Self(gate))
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl Drop for WorkerPermit {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::Release);
    }
}
#[cfg(all(not(test), not(target_arch = "wasm32")))]
fn worker_gate() -> std::sync::Arc<std::sync::atomic::AtomicBool> {
    static ACTIVE: std::sync::OnceLock<std::sync::Arc<std::sync::atomic::AtomicBool>> = std::sync::OnceLock::new();
    ACTIVE.get_or_init(Default::default).clone()
}
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn permit(_app: &SolveApp) -> Option<WorkerPermit> {
    #[cfg(test)]
    let gate = _app.forms_prototype.finish_worker_gate.clone();
    #[cfg(not(test))]
    let gate = worker_gate();
    WorkerPermit::acquire(gate)
}
pub(super) struct Pending {
    command: &'static str,
    revision: u64,
    cage: solidcraft_engine::geom::subdivision::Cage,
    feature: Option<u64>,
    done: std::sync::mpsc::Receiver<Result<solidcraft_engine::ComputedCommands, String>>,
}
pub(super) fn start(app: &mut SolveApp, ctx: &egui::Context) {
    if app.forms_prototype.pending.is_some() {
        return;
    }
    let params = match controls::finish_params(app) {
        Ok(p) => p,
        Err(error) => {
            app.set_status(error, true);
            return;
        }
    };
    command(app, ctx, "form.prototype.commit", params);
}
pub(super) fn pending_label(app: &SolveApp) -> &'static str {
    if app.forms_prototype.pending.as_ref().is_some_and(|job| job.command == "form.prototype.commit") {
        "forms.prototype.fitting"
    } else {
        "forms.prototype.previewing"
    }
}
pub(super) fn command(app: &mut SolveApp, ctx: &egui::Context, id: &'static str, params: serde_json::Value) {
    if app.forms_prototype.pending.is_some() {
        return;
    }
    app.status = None;
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(form) = &app.session.forms_prototype {
        // The worker owns this permit. Editor cancellation may discard its
        // receiver, but cannot release capacity or trigger another scratch copy.
        let Some(permit) = permit(app) else {
            app.set_status(app.tr("forms.prototype.command_worker_busy").to_owned(), true);
            return;
        };
        let revision = form.revision;
        let cage = form.cage.clone();
        let feature = form.editing_feature;
        let copy = app.session.scratch();
        let wake = ctx.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let given = params.clone();
        let spawned = std::thread::Builder::new().name("form-command".into()).spawn(move || {
            let _permit = permit;
            let result = copy.compute_commands(&[(id.into(), given)], None).map_err(|e| e.to_string());
            let _ = tx.send(result);
            wake.request_repaint();
        });
        match spawned {
            Ok(_) => app.forms_prototype.pending = Some(Pending { command: id, revision, cage, feature, done: rx }),
            Err(e) => app.set_status(e.to_string(), true),
        }
        return;
    }
    let _ = ctx;
    if run(app, id, params) {
        completed(app, id);
    }
}
/// Reuse a hovered transaction's existing worker instead of recomputing its
/// operation on click. Acceptance still verifies the full engine source context.
pub(super) fn await_command(
    app: &mut SolveApp,
    id: &'static str,
    done: std::sync::mpsc::Receiver<Result<solidcraft_engine::ComputedCommands, String>>,
) {
    if app.forms_prototype.pending.is_some() {
        return;
    }
    let Some(form) = &app.session.forms_prototype else { return };
    app.status = None;
    app.forms_prototype.pending =
        Some(Pending { command: id, revision: form.revision, cage: form.cage.clone(), feature: form.editing_feature, done });
}
fn completed(app: &mut SolveApp, id: &str) {
    if id == "form.prototype.extrude_drag" {
        // This worker path is the one-shot numeric action, not a pointer
        // gesture. Retain the old successful-action clearing, after acceptance.
        selection::cancel_region_work(app);
        app.forms_prototype.selected.clear();
        app.forms_prototype.elements.clear();
        app.forms_prototype.pivot = None;
        app.forms_prototype.selection = Default::default();
        app.forms_prototype.hover = None;
    }
    if id != "form.prototype.commit" {
        tools::completed(app, id);
    }
}
pub(super) fn tick(app: &mut SolveApp) {
    let Some(pending) = &app.forms_prototype.pending else {
        return;
    };
    let result = match pending.done.try_recv() {
        Ok(result) => result,
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        Err(_) => Err("Form computation stopped".into()),
    };
    let current = app
        .session
        .forms_prototype
        .as_ref()
        .is_some_and(|f| f.revision == pending.revision && f.cage == pending.cage && f.editing_feature == pending.feature);
    app.forms_prototype.pending = None;
    if !current {
        app.set_status(app.tr("forms.prototype.changed_during_fit").to_owned(), true);
        return;
    }
    let result = result.and_then(|prepared| app.session.accept_commands(prepared).map_err(|e| e.to_string()));
    match result {
        Ok(replies) => {
            for (id, reply) in replies {
                let _ = app.command_completed(&id, Ok(reply));
                completed(app, &id);
            }
        }
        Err(error) => app.set_status(error, true),
    }
}
/// Await a one-shot worker deterministically; never used by production frames.
#[cfg(test)]
pub(super) fn flush_for_test(app: &mut SolveApp) -> Result<(), String> {
    let Some(pending) = app.forms_prototype.pending.take() else { return Ok(()) };
    let result = pending.done.recv().map_err(|_| "Form test worker disconnected".to_owned())?;
    let (tx, done) = std::sync::mpsc::channel();
    tx.send(result).map_err(|_| "Form test result receiver disconnected".to_owned())?;
    app.forms_prototype.pending = Some(Pending { done, ..pending });
    tick(app);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn form_command_capacity_survives_receiver_drop_and_editor_reset() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[2,2,1],"levels":1})).unwrap();
        let gate = app.forms_prototype.finish_worker_gate.clone();
        let permit = WorkerPermit::acquire(gate.clone()).unwrap();
        let (entered_tx, entered) = std::sync::mpsc::channel();
        let (release_tx, release) = std::sync::mpsc::channel();
        let (done_tx, done) = std::sync::mpsc::channel();
        let old = std::thread::spawn(move || {
            let _permit = permit;
            entered_tx.send(()).unwrap();
            release.recv().unwrap();
            assert!(done_tx.send(Err("cancelled worker result".to_owned())).is_err(), "editor reset must have discarded the old receiver");
        });
        entered.recv().unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        app.forms_prototype.pending = Some(Pending {
            command: "form.prototype.extrude_drag",
            revision: form.revision,
            cage: form.cage.clone(),
            feature: form.editing_feature,
            done,
        });
        // Production uses a process-wide gate, independent of this State. The
        // explicit instance gate recreates that lifetime while isolating tests.
        app.forms_prototype = State { finish_worker_gate: gate.clone(), ..Default::default() };
        app.forms_prototype.elements.insert(Element::Face(0));
        selection::sync(&mut app);
        app.forms_prototype.pivot = Some(Vec3::new(3.0, 4.0, 5.0));
        let before = solidcraft_engine::recovery::capture_bytes(&app.session).unwrap();
        let elements = app.forms_prototype.elements.clone();
        let selected = app.forms_prototype.selected.clone();
        let pivot = app.forms_prototype.pivot;
        let params = json!({"faces":[0],"displacement":[0,0,2],"surface":true,"checkpoint":true});
        for _ in 0..3 {
            command(&mut app, &egui::Context::default(), "form.prototype.extrude_drag", params.clone());
            assert!(app.forms_prototype.pending.is_none());
            assert_eq!(
                app.status.as_ref().map(|(text, _, error)| (text.as_str(), *error)),
                Some((app.tr("forms.prototype.command_worker_busy"), true))
            );
            assert_eq!(solidcraft_engine::recovery::capture_bytes(&app.session).unwrap(), before);
            assert_eq!(app.forms_prototype.elements, elements);
            assert_eq!(app.forms_prototype.selected, selected);
            assert_eq!(app.forms_prototype.pivot, pivot);
            assert!(WorkerPermit::acquire(gate.clone()).is_none());
        }
        release_tx.send(()).unwrap();
        old.join().unwrap();
        command(&mut app, &egui::Context::default(), "form.prototype.extrude_drag", params);
        let next = app.forms_prototype.pending.take().unwrap();
        let result = next.done.recv().unwrap();
        assert!(result.is_ok());
        let (tx, done) = std::sync::mpsc::channel();
        tx.send(result).unwrap();
        app.forms_prototype.pending = Some(Pending { done, ..next });
        tick(&mut app);
        assert!(app.forms_prototype.pending.is_none());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], 1);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn form_command_permit_releases_when_a_worker_unwinds_or_a_spawn_closure_is_dropped() {
        let gate = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let permit = WorkerPermit::acquire(gate.clone()).unwrap();
        let failed = std::thread::spawn(move || {
            let _permit = permit;
            panic!("deliberate worker unwind");
        });
        assert!(failed.join().is_err());
        let permit = WorkerPermit::acquire(gate.clone()).unwrap();
        let rejected_spawn = move || {
            let _permit = permit;
        };
        assert!(WorkerPermit::acquire(gate.clone()).is_none());
        drop(rejected_spawn);
        assert!(WorkerPermit::acquire(gate).is_some());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn discarded_numeric_extrusion_worker_cannot_change_source_selection_or_pivot() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[2,2,1],"levels":1})).unwrap();
        app.forms_prototype.elements.insert(Element::Face(0));
        selection::sync(&mut app);
        app.forms_prototype.pivot = Some(Vec3::new(3.0, 4.0, 5.0));
        let before = solidcraft_engine::recovery::capture_bytes(&app.session).unwrap();
        let elements = app.forms_prototype.elements.clone();
        let selected = app.forms_prototype.selected.clone();
        let pivot = app.forms_prototype.pivot;
        command(
            &mut app,
            &egui::Context::default(),
            "form.prototype.extrude_drag",
            json!({"faces":[0],"displacement":[0,0,2],"surface":true,"checkpoint":true}),
        );
        // Removing the pending receiver is the existing cancellation boundary.
        // Drain it here only so this test leaves no detached computation running.
        let discarded = app.forms_prototype.pending.take().unwrap();
        assert!(discarded.done.recv().unwrap().is_ok());
        tick(&mut app);
        assert!(app.forms_prototype.pending.is_none());
        assert_eq!(solidcraft_engine::recovery::capture_bytes(&app.session).unwrap(), before);
        assert_eq!(app.forms_prototype.elements, elements);
        assert_eq!(app.forms_prototype.selected, selected);
        assert_eq!(app.forms_prototype.pivot, pivot);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn numeric_extrusion_worker_rejects_changed_document_even_with_the_same_form_revision() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[2,2,1],"levels":1})).unwrap();
        app.forms_prototype.elements.insert(Element::Face(0));
        selection::sync(&mut app);
        app.forms_prototype.pivot = Some(Vec3::new(3.0, 4.0, 5.0));
        let form_revision = app.session.forms_prototype.as_ref().unwrap().revision;
        let elements = app.forms_prototype.elements.clone();
        let selected = app.forms_prototype.selected.clone();
        let pivot = app.forms_prototype.pivot;
        command(
            &mut app,
            &egui::Context::default(),
            "form.prototype.extrude_drag",
            json!({"faces":[0],"displacement":[0,0,2],"surface":true,"checkpoint":true}),
        );
        app.session.execute("parameters.add", &json!({"name":"later","expression":"3 mm"})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().revision, form_revision);
        let changed = solidcraft_engine::recovery::capture_bytes(&app.session).unwrap();
        flush_for_test(&mut app).unwrap();
        assert!(app.status.as_ref().is_some_and(|(_, _, error)| *error));
        assert_eq!(solidcraft_engine::recovery::capture_bytes(&app.session).unwrap(), changed);
        assert_eq!(app.forms_prototype.elements, elements);
        assert_eq!(app.forms_prototype.selected, selected);
        assert_eq!(app.forms_prototype.pivot, pivot);
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn deferred_exact_insertion_waits_for_all_clicks_then_accepts_one_worker_checkpoint() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[10,10,1],"segments":[1,1,1],"levels":0})).unwrap();
        app.forms_prototype.controls.tolerance = "0.01 mm".into();
        let before = app.session.forms_prototype.as_ref().unwrap().definition();
        let face = before.cage.faces[0].clone();
        let a = [face[0].min(face[1]), face[0].max(face[1])];
        let b = [face[2].min(face[3]), face[2].max(face[3])];
        let descriptors = solidcraft_engine::forms_prototype::insertion_curves(&app.session, 0.01, &[]).unwrap();
        fit(&mut app);
        app.cam.pitch = std::f64::consts::FRAC_PI_2;
        let projection = crate::viewport::projection(&app, egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        let a = projection.to_screen(descriptors.iter().find(|curve| curve.edge == a).unwrap().at(0.3).unwrap()).unwrap();
        let b = projection.to_screen(descriptors.iter().find(|curve| curve.edge == b).unwrap().at(0.65).unwrap()).unwrap();
        let held = curves::pending_for_test(&mut app);
        tools::queue_point_for_test(&mut app, projection, a);
        tools::queue_point_for_test(&mut app, projection, b);
        tools::commit_points_for_test(&mut app, &egui::Context::default());
        assert!(app.forms_prototype.pending.is_none());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        held.send(Ok(descriptors)).unwrap();
        tools::tick_points_for_test(&mut app, &egui::Context::default());
        let pending = app.forms_prototype.pending.take().expect("deferred Enter must launch exactly one command worker after both clicks resolve");
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        let result = pending.done.recv().unwrap();
        assert!(result.is_ok());
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(result).unwrap();
        app.forms_prototype.pending = Some(Pending { done: rx, ..pending });
        tick(&mut app);
        let form = app.session.forms_prototype.as_ref().unwrap();
        assert!(form.hierarchy.is_some());
        assert_eq!(form.inspect()["undo"], 1);
        assert_eq!(tools::point_count_for_test(&app), 0);
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        assert!(app.session.doc.features.is_empty() && app.session.undo.is_empty());
    }

    #[test]
    fn saved_parametric_tolerance_reopens_and_finishes_without_losing_its_expression() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        let parameter = app.session.doc_mut().new_model_param("0.02", "mm");
        let expression = format!("{parameter} / 2");
        app.session.execute("form.prototype.create", &json!({"segments":[1,1,1],"size":10})).unwrap();
        app.session.execute("form.prototype.finish_options", &json!({"tolerance":expression})).unwrap();
        let feature = app.session.execute("form.prototype.commit", &json!({})).unwrap()["feature"].as_u64().unwrap();
        let saved = serde_json::to_value(app.session.doc.as_ref()).unwrap();
        let restored = serde_json::from_value(saved).unwrap();
        app.session.doc = std::sync::Arc::new(restored);
        app.session.execute("form.edit", &json!({"feature":feature})).unwrap();
        opened(&mut app);
        assert_eq!(app.forms_prototype.controls.tolerance, expression);
        assert_eq!(controls::tolerance(&app), Some(0.01));
        let params = controls::finish_params(&mut app).unwrap();
        assert!(params.get("tolerance").is_none());
        app.session.execute("form.prototype.commit", &params).unwrap();
        let solidcraft_engine::doc::FeatureKind::Form { tolerance, .. } = &app.session.doc.feature(feature).unwrap().kind else { panic!("Form") };
        assert_eq!(tolerance.as_str(), expression);
        app.session.doc_mut().params.iter_mut().find(|p| p.name == parameter).unwrap().expr = "0.04".into();
        assert_eq!(app.session.doc.eval(&expression, solidcraft_engine::doc::expr::Kind::Length).unwrap(), 0.02);
    }

    #[test]
    fn preferred_tolerance_stays_in_millimetres_for_non_mm_documents() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.doc_mut().units = "in".into();
        app.preferences.forms_cad_tolerance = 0.02;
        app.session.execute("form.prototype.create", &json!({"segments":[1,1,1],"size":10})).unwrap();
        opened(&mut app);
        assert_eq!(app.forms_prototype.controls.tolerance, "0.02 mm");
        assert_eq!(controls::tolerance(&app), Some(0.02));
        controls::finish_params(&mut app).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().finish_options.tolerance.as_deref(), Some("0.02 mm"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn dimension_worker_preserves_the_live_editor_and_refuses_stale_results() {
        for stale in [false, true] {
            let mut app = SolveApp::new(Default::default(), Default::default());
            app.session.execute("form.prototype.create", &json!({"dimensions":[10,10,10],"segments":[1,1,1],"levels":1})).unwrap();
            let point = app.session.forms_prototype.as_ref().unwrap().cage.points[0];
            app.session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[point.x-1.0,point.y,point.z]]})).unwrap();
            let before = app.session.forms_prototype.as_ref().unwrap().definition();
            let undo = app.session.forms_prototype.as_ref().unwrap().inspect()["undo"].as_u64().unwrap();
            command(&mut app, &egui::Context::default(), "form.prototype.parameters", json!({"dimensions":[14,10,10],"tolerance":0.02}));
            let mut pending = app.forms_prototype.pending.take().expect("dimension Apply must run on the native command worker");
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
            // Wait on the computation's channel, not on an assumed wall-clock duration.
            let result = pending.done.recv().unwrap();
            assert!(result.is_ok());
            let (tx, rx) = std::sync::mpsc::channel();
            tx.send(result).unwrap();
            pending.done = rx;
            app.forms_prototype.pending = Some(pending);
            if stale {
                app.session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[point.x-1.5,point.y,point.z]]})).unwrap();
            }
            let current = app.session.forms_prototype.as_ref().unwrap().definition();
            tick(&mut app);
            let form = app.session.forms_prototype.as_ref().unwrap();
            assert!(app.forms_prototype.pending.is_none());
            if stale {
                assert_eq!(form.definition(), current);
                assert_eq!(form.parameters.dimensions, [10.0; 3]);
            } else {
                assert_eq!(form.parameters.dimensions, [14.0, 10.0, 10.0]);
                assert_eq!(form.inspect()["undo"], undo + 1);
                app.session.execute("form.prototype.undo", &json!({})).unwrap();
                assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
            }
            assert!(app.session.doc.features.is_empty() && app.session.undo.is_empty());
        }
    }

    #[test]
    fn prepared_finish_is_atomic_and_stale_edits_are_kept() {
        for stale in [false, true] {
            let mut app = SolveApp::new(Default::default(), Default::default());
            app.ui.show_browser = false;
            app.ui.show_timeline = false;
            app.session.execute("form.prototype.create", &json!({"segments":[1,1,1],"size":10})).unwrap();
            let form = app.session.forms_prototype.as_ref().unwrap();
            let revision = form.revision;
            let cage = form.cage.clone();
            let prepared = app.session.scratch().compute_commands(&[("form.prototype.commit".into(), json!({"tolerance":0.05}))], None).unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            tx.send(Ok(prepared)).unwrap();
            app.forms_prototype.pending = Some(Pending { command: "form.prototype.commit", revision, cage, feature: None, done: rx });
            if stale {
                app.session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[-6,5,5]]})).unwrap();
            }
            tick(&mut app);
            assert_eq!(app.session.forms_prototype.is_some(), stale);
            assert_eq!(app.session.doc.features.len(), usize::from(!stale));
            assert_eq!(app.session.undo.len(), usize::from(!stale));
            assert!(!app.ui.show_browser && !app.ui.show_timeline);
            if !stale {
                app.session.execute("edit.undo", &json!({})).unwrap();
                assert!(app.session.doc.features.is_empty());
            }
        }
    }
    #[test]
    fn prepared_refinement_keeps_the_editor_open_and_refuses_stale_edits() {
        for stale in [false, true] {
            let mut app = SolveApp::new(Default::default(), Default::default());
            app.session.execute("form.prototype.create", &json!({"segments":[1,1,1],"size":10})).unwrap();
            let form = app.session.forms_prototype.as_ref().unwrap();
            let revision = form.revision;
            let cage = form.cage.clone();
            let prepared =
                app.session.scratch().compute_commands(&[("form.prototype.refine".into(), json!({"faces":[0],"counts":[2,2]}))], None).unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            tx.send(Ok(prepared)).unwrap();
            app.forms_prototype.pending = Some(Pending { command: "form.prototype.refine", revision, cage, feature: None, done: rx });
            if stale {
                app.session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[-6,5,5]]})).unwrap();
            }
            tick(&mut app);
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().hierarchy.is_some(), !stale);
            assert!(app.session.doc.features.is_empty());
        }
    }
}
