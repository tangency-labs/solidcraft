//! Fit and evaluate a finished Form away from the native window thread.
use super::*;
pub(super) struct Pending {
    revision: u64,
    cage: solidcraft_engine::geom::subdivision::Cage,
    feature: Option<u64>,
    done: std::sync::mpsc::Receiver<Result<solidcraft_engine::ComputedCommands, String>>,
}
pub(super) fn start(app: &mut SolveApp, ctx: &egui::Context) {
    if app.forms_prototype.pending.is_some() {
        return;
    }
    let params = match controls::commit_params(app) {
        Ok(p) => p,
        Err(error) => {
            app.set_status(error, true);
            return;
        }
    };
    app.status = None;
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(form) = &app.session.forms_prototype {
        let revision = form.revision;
        let cage = form.cage.clone();
        let feature = form.editing_feature;
        let copy = app.session.scratch();
        let wake = ctx.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let given = params.clone();
        let spawned = std::thread::Builder::new().name("finish-form".into()).spawn(move || {
            let result = copy.compute_commands(&[("form.prototype.commit".into(), given)], None).map_err(|e| e.to_string());
            let _ = tx.send(result);
            wake.request_repaint();
        });
        if spawned.is_ok() {
            app.forms_prototype.pending = Some(Pending { revision, cage, feature, done: rx });
            return;
        }
    }
    let _ = ctx;
    if run(app, "form.prototype.commit", params) {
        app.ui.show_browser = true;
        app.ui.show_timeline = true;
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
            app.session.forms_prototype = None;
            app.ui.show_browser = true;
            app.ui.show_timeline = true;
            for (id, reply) in replies {
                let _ = app.command_completed(&id, Ok(reply));
            }
        }
        Err(error) => app.set_status(error, true),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prepared_finish_is_atomic_and_stale_edits_are_kept() {
        for stale in [false, true] {
            let mut app = SolveApp::new(Default::default(), Default::default());
            app.session.execute("form.prototype.create", &json!({"segments":[1,1,1],"size":10})).unwrap();
            let form = app.session.forms_prototype.as_ref().unwrap();
            let revision = form.revision;
            let cage = form.cage.clone();
            let prepared = app.session.scratch().compute_commands(&[("form.prototype.commit".into(), json!({"tolerance":0.05}))], None).unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            tx.send(Ok(prepared)).unwrap();
            app.forms_prototype.pending = Some(Pending { revision, cage, feature: None, done: rx });
            if stale {
                app.session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[-6,5,5]]})).unwrap();
            }
            tick(&mut app);
            assert_eq!(app.session.forms_prototype.is_some(), stale);
            assert_eq!(app.session.doc.features.len(), usize::from(!stale));
            assert_eq!(app.session.undo.len(), usize::from(!stale));
            if !stale {
                app.session.execute("edit.undo", &json!({})).unwrap();
                assert!(app.session.doc.features.is_empty());
            }
        }
    }
}
