//! Whole-document invalidations delivered beside the window on the command worker.

use std::collections::BTreeMap;
use std::sync::{Arc, mpsc};

use solidcraft_engine::plugin::{AppState, DesignToken};

use crate::SolveApp;

const INTERVAL: f64 = 0.1;

struct Job {
    uid: u64,
    token: Option<DesignToken>,
    done: mpsc::Receiver<AppState>,
}

#[derive(Default)]
pub(crate) struct State {
    observed_uid: Option<u64>,
    observed: Option<DesignToken>,
    pending: Option<(Option<DesignToken>, f64)>,
    job: Option<Job>,
}

impl State {
    /// Keep the newest token without postponing a burst indefinitely.
    pub(crate) fn observe(&mut self, token: Option<DesignToken>, now: f64) {
        if self.observed == token {
            return;
        }
        self.invalidate(token, now);
    }

    fn invalidate(&mut self, token: Option<DesignToken>, now: f64) {
        self.observed = token.clone();
        let due = self.pending.as_ref().map_or(now + INTERVAL, |(_, due)| *due);
        self.pending = Some((token, due));
    }

    fn retry(&mut self, token: Option<DesignToken>, now: f64) {
        if self.pending.is_none() {
            self.pending = Some((token, now + INTERVAL));
        }
    }

    pub(crate) fn take_due(&mut self, now: f64) -> Option<Option<DesignToken>> {
        if self.pending.as_ref().is_some_and(|(_, due)| now >= *due) { self.pending.take().map(|(token, _)| token) } else { None }
    }
}

pub(crate) fn observe(app: &mut SolveApp) {
    let token = app.session.design_token();
    if app.plugin_events.observed_uid != Some(app.session.uid) {
        app.session.plugin_state.listener_data = Default::default();
        app.plugin_events.observed_uid = Some(app.session.uid);
        app.plugin_events.invalidate(Some(token), app.now);
    } else {
        app.plugin_events.observe(Some(token), app.now);
    }
}

pub(crate) fn command_completed(app: &mut SolveApp, id: &str, succeeded: bool) {
    observe(app);
    // Undo and redo also restore display-only history. Those successful host actions
    // invalidate listeners even when they restore the same modelling token.
    if succeeded && solidcraft_engine::find_command(id).is_some_and(|spec| matches!(spec.id, "edit.undo" | "edit.redo")) {
        app.plugin_events.invalidate(Some(app.session.design_token()), app.now);
    }
}

/// An integration point for the plugin panel host. Listener output is transient host data.
fn panel_data(app: &mut SolveApp, data: Arc<BTreeMap<String, serde_json::Value>>) {
    app.session.plugin_state.listener_data = data;
}

fn completed(app: &mut SolveApp, uid: u64, token: Option<DesignToken>, mut state: AppState) {
    let ids: std::collections::BTreeSet<_> =
        app.session.plugin_state.listener_failures.keys().chain(state.listener_failures.keys()).cloned().collect();
    for id in ids {
        // Revoking a grant while a listener runs keeps it revoked. A successful later
        // listener clears its previous failure, without adopting unrelated scratch state.
        if app.preferences.plugin_grants.get(&id) == state.grants.get(&id) && !app.preferences.disabled_plugins.contains(&id) {
            if let Some(error) = state.listener_failures.remove(&id) {
                app.session.plugin_state.listener_failures.insert(id, error);
            } else {
                app.session.plugin_state.listener_failures.remove(&id);
            }
        }
    }
    if uid == app.session.uid && token == Some(app.session.design_token()) {
        Arc::make_mut(&mut state.listener_data).retain(|key, _| {
            key.split_once('.').is_some_and(|(id, _)| {
                app.preferences.plugin_grants.get(id).is_some_and(|grant| grant.package_hash.is_some())
                    && app.preferences.plugin_grants.get(id) == state.grants.get(id)
                    && !app.preferences.disabled_plugins.contains(id)
            })
        });
        panel_data(app, state.listener_data);
    }
}

#[cfg(test)]
pub(crate) fn drain(app: &mut SolveApp) -> Result<(), String> {
    if let Some(job) = app.plugin_events.job.take() {
        let state = job.done.recv().map_err(|error| error.to_string())?;
        completed(app, job.uid, job.token, state);
    }
    Ok(())
}

pub(crate) fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    tick_with_spawner(app, ctx, |mut scratch, event| {
        let progress = Arc::new(solidcraft_engine::doc::progress::Progress::default());
        crate::control_job::spawn(None, progress, move || {
            solidcraft_engine::plugin::design_changed(&mut scratch, event);
            scratch.plugin_state
        })
    });
}

#[cfg(test)]
pub(crate) fn tick_with_spawner_for_test(
    app: &mut SolveApp,
    ctx: &egui::Context,
    spawn: impl FnOnce(solidcraft_engine::Session, Option<DesignToken>) -> Option<mpsc::Receiver<AppState>>,
) {
    tick_with_spawner(app, ctx, spawn);
}

fn tick_with_spawner(
    app: &mut SolveApp,
    ctx: &egui::Context,
    spawn: impl FnOnce(solidcraft_engine::Session, Option<DesignToken>) -> Option<mpsc::Receiver<AppState>>,
) {
    // Preferences can change during this frame, and another tab may hold an older policy.
    // Only synchronize when needed: the frame's normal policy refresh already ran in logic.
    if app.session.disabled_plugins != app.preferences.disabled_plugins
        || app.session.plugin_state.grants != app.preferences.plugin_grants
        || app.session.plugin_limits != crate::plugins::host_limits(&app.preferences)
    {
        crate::plugins::sync_policy(app);
    }
    if let Some(job) = app.plugin_events.job.take() {
        match job.done.try_recv() {
            Ok(state) => completed(app, job.uid, job.token, state),
            Err(mpsc::TryRecvError::Empty) => app.plugin_events.job = Some(job),
            Err(mpsc::TryRecvError::Disconnected) => {
                log::error!("design-changed command worker disconnected");
                app.plugin_events.retry(job.token, app.now);
            }
        }
    }
    // The live design remains owned by the window. Evaluation and file loads still under way
    // are not committed changes; their latest invalidation waits for the finished action.
    if app.applying.is_none() && app.deferred.is_none() && app.control_job.is_none() && !app.file_load.busy() {
        observe(app);
        if app.plugin_events.job.is_none()
            && let Some(token) = app.plugin_events.take_due(app.now)
        {
            let mut scratch = app.session.scratch();
            // Only this dispatch's results can be published. Cached outputs from removed or
            // newly unapproved packages must not return with an unrelated listener's result.
            scratch.plugin_state.listener_data = Default::default();
            if let Some(done) = spawn(scratch, token.clone()) {
                app.plugin_events.job = Some(Job { uid: app.session.uid, token, done });
            } else {
                app.plugin_events.retry(token, app.now);
            }
        }
    }
    if app.plugin_events.pending.is_some() || app.plugin_events.job.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}
