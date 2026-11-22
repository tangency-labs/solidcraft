//! Design and linked-file invalidations delivered beside the window on the command worker.

use std::collections::BTreeMap;
use std::sync::{Arc, mpsc};

use solidcraft_engine::plugin::{AppState, DesignToken, LinkChange};

use crate::SolveApp;

const INTERVAL: f64 = 0.1;

struct Job {
    uid: u64,
    notification: Notification,
    link_snapshot: Arc<BTreeMap<u64, LinkChange>>,
    done: mpsc::Receiver<AppState>,
}

#[derive(Clone)]
struct Notification {
    token: Option<DesignToken>,
    design_changed: bool,
    links: BTreeMap<u64, LinkChange>,
}

#[derive(Default)]
pub(crate) struct State {
    observed_uid: Option<u64>,
    observed: Option<DesignToken>,
    observed_links: Arc<BTreeMap<u64, LinkChange>>,
    pending: Option<(Notification, f64)>,
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
        let (pending, _) = self
            .pending
            .get_or_insert_with(|| (Notification { token: token.clone(), design_changed: false, links: BTreeMap::new() }, now + INTERVAL));
        pending.token = token;
        pending.design_changed = true;
    }

    fn observe_links(&mut self, links: Arc<BTreeMap<u64, LinkChange>>, now: f64) {
        if self.observed_links == links {
            return;
        }
        // Disk observations do not change the modelling token. Keep their invalidation
        // separate so existing design.changed listeners are not invoked by a link check.
        for (component, link) in links.iter() {
            if self.observed_links.get(component) != Some(link) {
                let (pending, _) = self.pending.get_or_insert_with(|| {
                    (Notification { token: self.observed.clone(), design_changed: false, links: BTreeMap::new() }, now + INTERVAL)
                });
                pending.links.insert(*component, link.clone());
            }
        }
        if let Some((pending, _)) = &mut self.pending {
            pending.links.retain(|component, _| links.contains_key(component));
        }
        self.observed_links = links;
    }

    fn retry(&mut self, mut notification: Notification, now: f64) {
        notification.links.retain(|component, link| self.observed_links.get(component) == Some(link));
        if let Some((pending, _)) = &mut self.pending {
            pending.design_changed |= notification.design_changed;
            for (component, link) in notification.links {
                pending.links.entry(component).or_insert(link);
            }
        } else if notification.design_changed || !notification.links.is_empty() {
            self.pending = Some((notification, now + INTERVAL));
        }
    }

    fn take_notification(&mut self, now: f64) -> Option<Notification> {
        if self.pending.as_ref().is_some_and(|(_, due)| now >= *due) { self.pending.take().map(|(notification, _)| notification) } else { None }
    }

    #[cfg(test)]
    pub(crate) fn take_due(&mut self, now: f64) -> Option<Option<DesignToken>> {
        self.take_notification(now).map(|notification| notification.token)
    }
}

pub(crate) fn observe(app: &mut SolveApp) {
    let token = app.session.design_token();
    if app.plugin_events.observed_uid != Some(app.session.uid) {
        app.session.plugin_state.listener_data = Default::default();
        app.plugin_events.observed_uid = Some(app.session.uid);
        app.plugin_events.pending = None;
        app.plugin_events.observed_links = Default::default();
        app.plugin_events.invalidate(Some(token), app.now);
    } else {
        app.plugin_events.observe(Some(token), app.now);
    }
    app.plugin_events.observe_links(app.session.plugin_state.link_changes.clone(), app.now);
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

fn completed(app: &mut SolveApp, job: Job, mut state: AppState) {
    if job.uid != app.session.uid {
        return;
    }
    let token = Some(app.session.design_token());
    if job.notification.token != token {
        // A modelling edit also supersedes a links-only result. Carry its links to the
        // latest token without replacing the edit's pending design invalidation.
        observe(app);
        let mut notification = job.notification;
        notification.token = token;
        app.plugin_events.retry(notification, app.now);
        return;
    }
    // A disk check can supersede the worker without changing the modelling token. Preserve
    // any whole-design invalidation too, so rejecting this output cannot lose its delivery.
    if job.link_snapshot != app.session.plugin_state.link_changes {
        app.plugin_events.retry(job.notification, app.now);
        return;
    }
    let ids: std::collections::BTreeSet<_> =
        app.session.plugin_state.listener_failures.keys().chain(state.listener_failures.keys()).cloned().collect();
    for id in ids {
        // Revoking a grant while a listener runs keeps it revoked. Each successful entry
        // clears its own previous failure, without adopting unrelated scratch state.
        if app.preferences.plugin_grants.get(&id) == state.grants.get(&id) && !app.preferences.disabled_plugins.contains(&id) {
            if let Some(failures) = state.listener_entry_failures.remove(&id) {
                app.session.plugin_state.listener_entry_failures.insert(id.clone(), failures);
            } else {
                app.session.plugin_state.listener_entry_failures.remove(&id);
            }
            if let Some(error) = state.listener_failures.remove(&id) {
                app.session.plugin_state.listener_failures.insert(id, error);
            } else {
                app.session.plugin_state.listener_failures.remove(&id);
            }
        }
    }
    if job.notification.token == Some(app.session.design_token()) {
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
        completed(app, job, state);
    }
    Ok(())
}

pub(crate) fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    tick_with_spawner(app, ctx, |mut scratch, notification| {
        let progress = Arc::new(solidcraft_engine::doc::progress::Progress::default());
        crate::control_job::spawn(None, progress, move || {
            let links: Vec<_> = notification.links.into_values().collect();
            solidcraft_engine::plugin::design_notifications(&mut scratch, notification.token, notification.design_changed, &links);
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
    tick_with_spawner(app, ctx, |scratch, notification| spawn(scratch, notification.token));
}

fn tick_with_spawner(
    app: &mut SolveApp,
    ctx: &egui::Context,
    spawn: impl FnOnce(solidcraft_engine::Session, Notification) -> Option<mpsc::Receiver<AppState>>,
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
            Ok(state) => completed(app, job, state),
            Err(mpsc::TryRecvError::Empty) => app.plugin_events.job = Some(job),
            Err(mpsc::TryRecvError::Disconnected) => {
                log::error!("design-changed command worker disconnected");
                if job.uid == app.session.uid {
                    app.plugin_events.retry(job.notification, app.now);
                }
            }
        }
    }
    // The live design remains owned by the window. Evaluation and file loads still under way
    // are not committed changes; their latest invalidation waits for the finished action.
    if app.applying.is_none() && app.deferred.is_none() && app.control_job.is_none() && !app.file_load.busy() {
        observe(app);
        if app.plugin_events.job.is_none()
            && let Some(notification) = app.plugin_events.take_notification(app.now)
        {
            // The worker replaces outputs only for invoked listeners and prunes caches of
            // packages without current approval. Other event subscriptions keep their data.
            let scratch = app.session.scratch();
            let retry = notification.clone();
            if let Some(done) = spawn(scratch, notification) {
                app.plugin_events.job =
                    Some(Job { uid: app.session.uid, notification: retry, link_snapshot: app.session.plugin_state.link_changes.clone(), done });
            } else {
                app.plugin_events.retry(retry, app.now);
            }
        }
    }
    if app.plugin_events.pending.is_some() || app.plugin_events.job.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}
